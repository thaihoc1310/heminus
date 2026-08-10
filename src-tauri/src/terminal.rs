use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
#[cfg(unix)]
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

use heminus_domain::{EnvironmentVariable, Host};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, EventTarget, Manager, State, WebviewWindow};
use uuid::Uuid;

use crate::AppState;

struct TerminalSession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    history_id: Uuid,
    event_sink: Arc<Mutex<TerminalEventSink>>,
    supervisor: crate::platform::ProcessSupervisor,
    _artifacts: crate::ssh_runtime::ConnectionArtifacts,
}

struct TerminalEventSink {
    destination: Option<TerminalEventDestination>,
    replay: VecDeque<u8>,
    generation: u64,
}

#[derive(Clone)]
enum TerminalEventDestination {
    Channel(Channel<TerminalEvent>),
    Window {
        app: AppHandle,
        label: String,
        event_name: String,
    },
}

impl TerminalEventDestination {
    fn send(&self, event: TerminalEvent) -> Result<(), ()> {
        match self {
            Self::Channel(channel) => channel.send(event).map_err(|_| ()),
            Self::Window {
                app,
                label,
                event_name,
            } => app
                .emit_to(
                    EventTarget::webview_window(label.clone()),
                    event_name,
                    event,
                )
                .map_err(|_| ()),
        }
    }
}

const TERMINAL_REPLAY_LIMIT: usize = 2 * 1024 * 1024;

pub struct TerminalManager {
    sessions: Arc<Mutex<HashMap<Uuid, TerminalSession>>>,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        let Ok(mut sessions) = self.sessions.lock() else {
            return;
        };
        let database = heminus_storage::Database::open_default().ok();
        for (_, mut session) in sessions.drain() {
            session.supervisor.terminate();
            let _ = session.killer.kill();
            if let Some(database) = database.as_ref() {
                let _ = database.finish_session(
                    session.history_id,
                    heminus_domain::SessionStatus::Disconnected,
                );
            }
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TerminalEvent {
    Output { bytes: Vec<u8> },
    Exit,
    Disconnect,
    Error { message: String },
}

fn publish_terminal_output(event_sink: &Arc<Mutex<TerminalEventSink>>, bytes: &[u8]) {
    let (destination, generation) = {
        let Ok(mut sink) = event_sink.lock() else {
            return;
        };
        sink.replay.extend(bytes);
        let excess = sink.replay.len().saturating_sub(TERMINAL_REPLAY_LIMIT);
        if excess > 0 {
            sink.replay.drain(..excess);
        }
        (sink.destination.clone(), sink.generation)
    };

    let event = TerminalEvent::Output {
        bytes: bytes.to_vec(),
    };
    if destination.is_some_and(|destination| destination.send(event).is_err())
        && let Ok(mut sink) = event_sink.lock()
        && sink.generation == generation
    {
        sink.destination = None;
    }
}

fn publish_terminal_event(event_sink: &Arc<Mutex<TerminalEventSink>>, event: TerminalEvent) {
    let (destination, generation) = {
        let Ok(sink) = event_sink.lock() else {
            return;
        };
        (sink.destination.clone(), sink.generation)
    };

    if destination.is_some_and(|destination| destination.send(event).is_err())
        && let Ok(mut sink) = event_sink.lock()
        && sink.generation == generation
    {
        sink.destination = None;
    }
}

fn is_local_host(host: &Host) -> bool {
    matches!(
        host.address.trim().to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost" | "::1"
    )
}

#[cfg(unix)]
fn successful_command_prompt(shell: &str, original: Option<&str>) -> Option<String> {
    if Path::new(shell)
        .file_name()
        .and_then(|value| value.to_str())
        != Some("bash")
    {
        return None;
    }
    let reporter = r#"__heminus_status=$?; builtin printf '\033]633;D;%s\007' "$__heminus_status"; (exit "$__heminus_status")"#;
    Some(
        match original.map(str::trim).filter(|value| !value.is_empty()) {
            Some(original) => format!("{reporter}; {original}"),
            None => reporter.into(),
        },
    )
}

fn append_remote_login_target(command: &mut CommandBuilder, username: &str, address: &str) {
    command.arg("--");
    command.arg(format!("{username}@{address}"));
}

fn local_shell_command(environment: &[EnvironmentVariable]) -> Result<CommandBuilder, String> {
    let shell = crate::platform::local_shell()?;
    let mut command = CommandBuilder::new(&shell.executable);
    for argument in shell.arguments {
        command.arg(argument);
    }
    command.cwd(crate::platform::home_dir()?);
    for variable in environment {
        command.env(&variable.name, &variable.value);
    }
    #[cfg(unix)]
    {
        let original_prompt_command = environment
            .iter()
            .find(|variable| variable.name == "PROMPT_COMMAND")
            .map(|variable| variable.value.clone())
            .or_else(|| std::env::var("PROMPT_COMMAND").ok());
        if let Some(prompt_command) = successful_command_prompt(
            &shell.executable.to_string_lossy(),
            original_prompt_command.as_deref(),
        ) {
            command.env("PROMPT_COMMAND", prompt_command);
        }
    }
    Ok(command)
}

#[tauri::command]
pub fn terminal_open(
    manager: State<'_, TerminalManager>,
    app_state: State<'_, AppState>,
    rows: u16,
    cols: u16,
    host: Option<heminus_domain::Host>,
    session_title: Option<String>,
    on_event: Channel<TerminalEvent>,
) -> Result<Uuid, String> {
    let history_host_id = host.as_ref().map(|value| value.id);
    let history_title = session_title
        .filter(|value| !value.trim().is_empty())
        .or_else(|| host.as_ref().map(|value| value.label.clone()))
        .unwrap_or_else(|| "Local Terminal".into());
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: rows.max(2),
            cols: cols.max(2),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| error.to_string())?;

    let mut connection_artifacts = crate::ssh_runtime::ConnectionArtifacts::default();
    let mut command = if let Some(host) = host {
        host.validate().map_err(|error| error.to_string())?;
        if is_local_host(&host) {
            local_shell_command(&host.environment)?
        } else {
            let (identity, host_arguments, credential_environment) = {
                let database = app_state
                    .database
                    .lock()
                    .map_err(|_| "database lock poisoned".to_string())?;
                let identity = host
                    .identity_id
                    .map(|id| {
                        database
                            .find_identity(id)
                            .map_err(|error| error.to_string())?
                            .ok_or_else(|| "The selected SSH identity no longer exists".to_string())
                    })
                    .transpose()?;
                let host_arguments = app_state.ssh.host_arguments(&database, &host)?;
                let credential_environment = crate::credential::connection_askpass_environment(
                    &database,
                    &host,
                    identity.as_ref(),
                )?;
                (identity, host_arguments, credential_environment)
            };
            connection_artifacts = app_state.ssh.connection_artifacts(&host_arguments);
            let username = identity
                .as_ref()
                .and_then(|value| value.username.as_deref())
                .unwrap_or(&host.username);
            let mut ssh = CommandBuilder::new(crate::platform::ssh_executable()?);
            for argument in app_state.ssh.arguments() {
                ssh.arg(argument);
            }
            for argument in host_arguments {
                ssh.arg(argument);
            }
            ssh.env_remove("SSH_AUTH_SOCK");
            ssh.env_remove("SSH_ASKPASS");
            ssh.env_remove("SSH_ASKPASS_REQUIRE");
            ssh.env_remove(crate::credential::askpass_candidates_env());
            if let Some((askpass, candidates)) = credential_environment {
                ssh.env("SSH_ASKPASS", askpass);
                ssh.env("SSH_ASKPASS_REQUIRE", "force");
                ssh.env(crate::credential::askpass_candidates_env(), candidates);
                #[cfg(unix)]
                if std::env::var_os("DISPLAY").is_none() {
                    ssh.env("DISPLAY", "heminus:0");
                }
            }
            ssh.arg("-p");
            ssh.arg(host.port.to_string());
            ssh.arg("-o");
            ssh.arg("ServerAliveInterval=30");
            ssh.arg("-o");
            ssh.arg("ServerAliveCountMax=3");
            ssh.arg("-o");
            ssh.arg("TCPKeepAlive=yes");
            ssh.arg("-tt");
            if let Some(key_path) = identity
                .as_ref()
                .filter(|value| value.kind == heminus_domain::IdentityKind::KeyFile)
                .and_then(|value| value.key_path.as_deref())
            {
                ssh.arg("-o");
                ssh.arg("IdentitiesOnly=yes");
                ssh.arg("-i");
                ssh.arg(key_path);
            } else if let Some(identity) = identity
                .as_ref()
                .filter(|value| value.kind == heminus_domain::IdentityKind::Password)
            {
                if !identity.secret_stored {
                    return Err("This password identity does not have a stored password".into());
                }
                ssh.arg("-o");
                ssh.arg("PreferredAuthentications=keyboard-interactive,password");
                ssh.arg("-o");
                ssh.arg("PubkeyAuthentication=no");
                ssh.arg("-o");
                ssh.arg("NumberOfPasswordPrompts=1");
            }
            append_remote_login_target(
                &mut ssh,
                username,
                crate::ssh_runtime::effective_address(&host),
            );
            ssh
        }
    } else {
        local_shell_command(&[])?
    };
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    if let Ok(language) = std::env::var("LANG") {
        command.env("LANG", language);
    }

    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| error.to_string())?;
    let supervisor = match crate::platform::ProcessSupervisor::attach(child.process_id()) {
        Ok(supervisor) => supervisor,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let session_killer = child.clone_killer();
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| error.to_string())?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| error.to_string())?;
    let id = Uuid::new_v4();
    let sessions = Arc::clone(&manager.sessions);
    let event_sink = Arc::new(Mutex::new(TerminalEventSink {
        destination: Some(TerminalEventDestination::Channel(on_event)),
        replay: VecDeque::new(),
        generation: 0,
    }));
    let history_id = match app_state
        .database
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?
        .start_session(history_host_id, history_title)
    {
        Ok(id) => id,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.to_string());
        }
    };

    sessions
        .lock()
        .map_err(|_| "terminal lock poisoned")?
        .insert(
            id,
            TerminalSession {
                master: pair.master,
                writer,
                killer: session_killer,
                history_id,
                event_sink: Arc::clone(&event_sink),
                supervisor,
                _artifacts: connection_artifacts,
            },
        );

    thread::Builder::new()
        .name(format!("heminus-pty-{id}"))
        .spawn(move || {
            let mut buffer = vec![0_u8; 16 * 1024];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        publish_terminal_output(&event_sink, &buffer[..read]);
                    }
                    Err(error) => {
                        let normal_close = matches!(
                            error.kind(),
                            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::UnexpectedEof
                        ) || error.raw_os_error() == Some(5);
                        if !normal_close {
                            publish_terminal_event(
                                &event_sink,
                                TerminalEvent::Error {
                                    message: error.to_string(),
                                },
                            );
                        }
                        break;
                    }
                }
            }
            let _ = child.wait();
            if let Ok(database) = heminus_storage::Database::open_default() {
                let _ = database
                    .finish_session(history_id, heminus_domain::SessionStatus::Disconnected);
            }
            if let Ok(mut sessions) = sessions.lock() {
                sessions.remove(&id);
            }
            publish_terminal_event(&event_sink, TerminalEvent::Exit);
        })
        .map_err(|error| error.to_string())?;

    Ok(id)
}

#[tauri::command]
pub fn terminal_attach(
    manager: State<'_, TerminalManager>,
    id: Uuid,
    window: WebviewWindow,
) -> Result<bool, String> {
    let destination = TerminalEventDestination::Window {
        app: window.app_handle().clone(),
        label: window.label().to_string(),
        event_name: format!("terminal-session-{id}"),
    };
    let event_sink = {
        let sessions = manager
            .sessions
            .lock()
            .map_err(|_| "terminal lock poisoned")?;
        Arc::clone(
            &sessions
                .get(&id)
                .ok_or_else(|| "terminal session not found".to_string())?
                .event_sink,
        )
    };
    let (replay, generation, previous_destination) = {
        let mut sink = event_sink
            .lock()
            .map_err(|_| "terminal event sink lock poisoned")?;
        sink.generation = sink.generation.wrapping_add(1);
        let generation = sink.generation;
        let previous_destination = sink.destination.replace(destination.clone());
        let replay = sink.replay.iter().copied().collect::<Vec<_>>();
        (replay, generation, previous_destination)
    };
    drop(previous_destination);

    if !replay.is_empty()
        && destination
            .send(TerminalEvent::Output { bytes: replay })
            .is_err()
    {
        if let Ok(mut sink) = event_sink.lock()
            && sink.generation == generation
        {
            sink.destination = None;
        }
        return Err("Could not attach the detached terminal output".to_string());
    }

    Ok(true)
}

#[tauri::command]
pub fn terminal_write(
    manager: State<'_, TerminalManager>,
    id: Uuid,
    bytes: Vec<u8>,
) -> Result<(), String> {
    let mut sessions = manager
        .sessions
        .lock()
        .map_err(|_| "terminal lock poisoned")?;
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| "terminal session not found".to_string())?;
    session
        .writer
        .write_all(&bytes)
        .and_then(|_| session.writer.flush())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn terminal_resize(
    manager: State<'_, TerminalManager>,
    id: Uuid,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    let sessions = manager
        .sessions
        .lock()
        .map_err(|_| "terminal lock poisoned")?;
    let session = sessions
        .get(&id)
        .ok_or_else(|| "terminal session not found".to_string())?;
    session
        .master
        .resize(PtySize {
            rows: rows.max(2),
            cols: cols.max(2),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn terminal_close(
    manager: State<'_, TerminalManager>,
    app_state: State<'_, AppState>,
    id: Uuid,
) -> Result<bool, String> {
    let removed = manager
        .sessions
        .lock()
        .map_err(|_| "terminal lock poisoned")?
        .remove(&id);
    let Some(mut session) = removed else {
        return Ok(false);
    };
    session.supervisor.terminate();
    let _ = session.killer.kill();
    app_state
        .database
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?
        .finish_session(
            session.history_id,
            heminus_domain::SessionStatus::Disconnected,
        )
        .map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
pub fn terminal_disconnect_history(
    manager: State<'_, TerminalManager>,
    app_state: State<'_, AppState>,
    history_id: Uuid,
) -> Result<bool, String> {
    let removed = {
        let mut sessions = manager
            .sessions
            .lock()
            .map_err(|_| "terminal lock poisoned")?;
        let runtime_id = sessions
            .iter()
            .find_map(|(id, session)| (session.history_id == history_id).then_some(*id));
        runtime_id.and_then(|id| sessions.remove(&id))
    };
    let Some(mut session) = removed else {
        return Ok(false);
    };
    let _ = session.killer.kill();
    app_state
        .database
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?
        .finish_session(history_id, heminus_domain::SessionStatus::Disconnected)
        .map_err(|error| error.to_string())?;
    publish_terminal_event(&session.event_sink, TerminalEvent::Disconnect);
    Ok(true)
}

#[tauri::command]
pub fn terminal_rename(
    manager: State<'_, TerminalManager>,
    app_state: State<'_, AppState>,
    id: Uuid,
    title: String,
) -> Result<bool, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Terminal name cannot be empty".into());
    }
    if title.chars().count() > 120 {
        return Err("Terminal name cannot exceed 120 characters".into());
    }
    let history_id = manager
        .sessions
        .lock()
        .map_err(|_| "terminal lock poisoned")?
        .get(&id)
        .map(|session| session.history_id);
    let Some(history_id) = history_id else {
        return Ok(false);
    };
    app_state
        .database
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?
        .rename_session(history_id, title)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_hosts_use_a_local_shell_with_their_profile_environment() {
        let mut host = Host::new("Local profile", "127.0.0.1", "local");
        host.environment.push(EnvironmentVariable {
            name: "hi".into(),
            value: "ha".into(),
        });

        assert!(is_local_host(&host));
        let command = local_shell_command(&host.environment).unwrap();
        assert_eq!(
            command.get_env("hi").and_then(|value| value.to_str()),
            Some("ha")
        );
        #[cfg(unix)]
        assert!(command.get_argv().iter().any(|argument| argument == "-l"));
        #[cfg(windows)]
        assert!(
            command
                .get_argv()
                .iter()
                .any(|argument| argument == "-NoLogo")
        );
    }

    #[test]
    fn remote_addresses_are_not_treated_as_local_profiles() {
        assert!(!is_local_host(&Host::new("Remote", "192.0.2.10", "deploy")));
    }

    #[test]
    #[cfg(unix)]
    fn bash_prompt_reports_exit_status_without_discarding_an_existing_hook() {
        let prompt = successful_command_prompt("/bin/bash", Some("update_terminal_title"))
            .expect("bash integration");
        assert!(prompt.contains("633;D;%s"));
        assert!(prompt.ends_with("; update_terminal_title"));
        assert!(successful_command_prompt("/bin/zsh", None).is_none());
    }

    #[test]
    fn remote_sessions_use_the_servers_interactive_login() {
        let mut command = CommandBuilder::new("ssh");
        append_remote_login_target(&mut command, "deploy", "192.0.2.10");
        let arguments = command
            .get_argv()
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(arguments, ["ssh", "--", "deploy@192.0.2.10"]);
    }
}
