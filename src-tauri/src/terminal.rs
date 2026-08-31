use std::collections::{HashMap, VecDeque};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use heminus_domain::{EnvironmentVariable, Host};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::AppState;

struct TerminalSession {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    /// True for SSH sessions, whose local processes are all Heminus transport.
    remote: bool,
    history_id: Uuid,
    event_sink: Arc<Mutex<TerminalEventSink>>,
    supervisor: crate::platform::ProcessSupervisor,
    _artifacts: crate::ssh_runtime::ConnectionArtifacts,
}

struct TerminalEventSink {
    destination: Option<Channel<InvokeResponseBody>>,
    replay: VecDeque<u8>,
    generation: u64,
}

const TERMINAL_REPLAY_LIMIT: usize = 2 * 1024 * 1024;

/// Replay is delivered in pieces so re-attaching never builds one huge message.
const TERMINAL_REPLAY_CHUNK: usize = 256 * 1024;

#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn terminal_clipboard_write(
    app: AppHandle,
    text: String,
    primary: bool,
) -> Result<(), String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let selection = if primary {
            &gdk::SELECTION_PRIMARY
        } else {
            &gdk::SELECTION_CLIPBOARD
        };
        let clipboard = gtk::Clipboard::get(selection);
        clipboard.set_text(&text);
        if !primary {
            clipboard.store();
        }
        let _ = sender.send(());
    })
    .map_err(|error| format!("Could not access the Linux clipboard: {error}"))?;
    receiver
        .await
        .map_err(|_| "The Linux clipboard operation was cancelled".to_string())
}

#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub async fn terminal_clipboard_write(
    _app: AppHandle,
    _text: String,
    _primary: bool,
) -> Result<(), String> {
    Err("Native terminal clipboard access is only available on Linux".to_string())
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn terminal_clipboard_read(
    app: AppHandle,
    primary: bool,
) -> Result<Option<String>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let selection = if primary {
            &gdk::SELECTION_PRIMARY
        } else {
            &gdk::SELECTION_CLIPBOARD
        };
        gtk::Clipboard::get(selection).request_text(move |_, text| {
            let _ = sender.send(text.map(str::to_owned));
        });
    })
    .map_err(|error| format!("Could not access the Linux clipboard: {error}"))?;
    receiver
        .await
        .map_err(|_| "The Linux clipboard operation was cancelled".to_string())
}

#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub async fn terminal_clipboard_read(
    _app: AppHandle,
    _primary: bool,
) -> Result<Option<String>, String> {
    Err("Native terminal clipboard access is only available on Linux".to_string())
}

pub struct TerminalManager {
    sessions: Arc<Mutex<HashMap<Uuid, TerminalSession>>>,
    /// Shared with `AppState`.
    ///
    /// Closing a terminal used to open a fresh SQLite connection, which reruns
    /// the whole migration batch and contends with the live one for the write
    /// lock, just to stamp one row as disconnected.
    database: Arc<Mutex<heminus_storage::Database>>,
}

impl TerminalManager {
    pub fn new(database: Arc<Mutex<heminus_storage::Database>>) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            database,
        }
    }
}

/// Marks a session finished, ignoring a poisoned lock or a closed database.
fn finish_session_quietly(database: &Mutex<heminus_storage::Database>, history_id: Uuid) {
    if let Ok(database) = database.lock() {
        let _ = database.finish_session(history_id, heminus_domain::SessionStatus::Disconnected);
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        let Ok(mut sessions) = self.sessions.lock() else {
            return;
        };
        for (_, mut session) in sessions.drain() {
            session.supervisor.terminate();
            let _ = session.killer.kill();
            finish_session_quietly(&self.database, session.history_id);
        }
    }
}

/// Everything except the output itself, which travels as raw bytes.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TerminalEvent {
    Exit,
    Disconnect,
    Error {
        message: String,
    },
    /// The legs of the connection, in order, announced before it starts.
    Hops {
        labels: Vec<String>,
    },
    /// One line of a hop's OpenSSH log, for the connecting overlay.
    Log {
        hop: usize,
        level: LogLevel,
        message: String,
        stage: Option<ConnectionStage>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

/// How far the SSH handshake has progressed, mirrored by the connecting screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionStage {
    Connecting,
    Handshake,
    Authenticating,
    Authenticated,
    Ready,
    Failed,
}

/// Classifies one line of `ssh -v -E <file>` output.
///
/// `-E` keeps the handshake chatter out of the terminal, so this is the only
/// place a failure such as "Connection refused" can still reach the person.
fn classify_connection_log(line: &str) -> Option<(LogLevel, String, Option<ConnectionStage>)> {
    let line = line.trim_end_matches(['\r', '\n']).trim();
    if line.is_empty() {
        return None;
    }
    let (level, message) = match line.split_once(": ") {
        Some((prefix, rest)) if prefix.starts_with("debug") => (LogLevel::Debug, rest),
        _ => (LogLevel::Info, line),
    };
    let lowered = message.to_ascii_lowercase();
    // Internal plumbing: it only names Heminus's own temporary files and the
    // command that builds the next hop, both of which the screen already shows.
    const NOISE: [&str; 3] = [
        "reading configuration data",
        "executing proxy command",
        "applying options for",
    ];
    if NOISE.iter().any(|prefix| lowered.starts_with(prefix)) {
        return None;
    }
    let failed = [
        "permission denied",
        "connection refused",
        "connection timed out",
        "connection closed by",
        "closed by remote host",
        "no route to host",
        "could not resolve hostname",
        "host key verification failed",
        "name or service not known",
        "too many authentication failures",
        "operation timed out",
        "network is unreachable",
        "not responding",
        "broken pipe",
        "no matching host key",
        "unable to negotiate",
        "heminus proxy:",
    ]
    .iter()
    .any(|marker| lowered.contains(marker));
    if failed {
        return Some((
            LogLevel::Error,
            message.to_string(),
            Some(ConnectionStage::Failed),
        ));
    }
    // Heminus pins unknown keys on first use, so report that as news rather
    // than as OpenSSH's generic warning.
    if lowered.contains("permanently added") {
        return Some((LogLevel::Info, message.to_string(), None));
    }
    if lowered.contains("warning:") {
        return Some((LogLevel::Warning, message.to_string(), None));
    }
    let stage = if lowered.starts_with("connecting to") {
        Some(ConnectionStage::Connecting)
    } else if lowered.starts_with("connection established") {
        Some(ConnectionStage::Handshake)
    } else if lowered.starts_with("authenticating to") || lowered.starts_with("next authentication")
    {
        Some(ConnectionStage::Authenticating)
    } else if lowered.starts_with("authentication succeeded")
        || lowered.starts_with("authenticated")
    {
        Some(ConnectionStage::Authenticated)
    } else if lowered.starts_with("entering interactive session") || lowered.starts_with("pledge: ")
    {
        Some(ConnectionStage::Ready)
    } else {
        None
    };
    Some((level, message.to_string(), stage))
}

/// Streams one hop's OpenSSH `-E` log file into the session channel as it grows.
fn spawn_connection_log_reader(
    hop: usize,
    path: PathBuf,
    event_sink: Arc<Mutex<TerminalEventSink>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) {
    let _ = thread::Builder::new()
        .name("heminus-ssh-log".into())
        .spawn(move || {
            use std::sync::atomic::Ordering;
            let mut offset = 0_u64;
            let mut remainder = String::new();
            // Poll briskly while the handshake is on screen, then back off: an
            // established session only writes here when something goes wrong.
            let mut interval = std::time::Duration::from_millis(60);
            loop {
                let finished = stop.load(Ordering::Relaxed);
                if let Ok(mut file) = std::fs::File::open(&path)
                    && file.seek(std::io::SeekFrom::Start(offset)).is_ok()
                {
                    let mut chunk = String::new();
                    if let Ok(read) = file.read_to_string(&mut chunk)
                        && read > 0
                    {
                        offset += read as u64;
                        remainder.push_str(&chunk);
                        while let Some(index) = remainder.find('\n') {
                            let line = remainder[..index].to_string();
                            remainder.drain(..=index);
                            if let Some((level, message, stage)) = classify_connection_log(&line) {
                                if matches!(
                                    stage,
                                    Some(ConnectionStage::Ready | ConnectionStage::Failed)
                                ) {
                                    // Nothing is watching the handshake any
                                    // more; this only has to catch a late
                                    // disconnect message.
                                    interval = std::time::Duration::from_secs(2);
                                }
                                publish_terminal_event(
                                    &event_sink,
                                    TerminalEvent::Log {
                                        hop,
                                        level,
                                        message,
                                        stage,
                                    },
                                );
                            }
                        }
                    }
                }
                if finished {
                    if let Some((level, message, stage)) = classify_connection_log(&remainder) {
                        publish_terminal_event(
                            &event_sink,
                            TerminalEvent::Log {
                                hop,
                                level,
                                message,
                                stage,
                            },
                        );
                    }
                    return;
                }
                thread::sleep(interval);
            }
        });
}

/// Drops the destination unless a re-attach already replaced it.
///
/// The generation check keeps a failure from a send issued *before* a re-attach
/// from tearing down the destination that replaced it.
fn clear_failed_destination(event_sink: &Arc<Mutex<TerminalEventSink>>, generation: u64) {
    if let Ok(mut sink) = event_sink.lock()
        && sink.generation == generation
    {
        sink.destination = None;
    }
}

/// Sends terminal output as raw bytes.
///
/// A `Vec<u8>` in a serialized event becomes a JSON array of decimal numbers —
/// a 16 KiB read turns into ~75 KB of text that the webview then has to parse
/// back into a boxed number array. `Raw` hands the same bytes to JavaScript as
/// an `ArrayBuffer` instead.
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

    if destination.is_some_and(|destination| {
        destination
            .send(InvokeResponseBody::Raw(bytes.to_vec()))
            .is_err()
    }) {
        clear_failed_destination(event_sink, generation);
    }
}

/// Sends a control event as JSON on the same channel as the output.
///
/// Sharing one channel is what keeps ordering intact: Tauri numbers every
/// message and the JavaScript side dispatches them in that order, so `Exit`
/// cannot overtake the output that preceded it.
fn publish_terminal_event(event_sink: &Arc<Mutex<TerminalEventSink>>, event: TerminalEvent) {
    let Ok(json) = serde_json::to_string(&event) else {
        return;
    };
    let (destination, generation) = {
        let Ok(sink) = event_sink.lock() else {
            return;
        };
        (sink.destination.clone(), sink.generation)
    };

    if destination
        .is_some_and(|destination| destination.send(InvokeResponseBody::Json(json)).is_err())
    {
        clear_failed_destination(event_sink, generation);
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

fn local_shell_command(
    environment: &[EnvironmentVariable],
    cwd: Option<&Path>,
) -> Result<CommandBuilder, String> {
    let shell = crate::platform::local_shell()?;
    let mut command = CommandBuilder::new(&shell.executable);
    for argument in shell.arguments {
        command.arg(argument);
    }
    match cwd.filter(|path| path.is_dir()) {
        Some(path) => command.cwd(path),
        None => command.cwd(crate::platform::home_dir()?),
    }
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

#[allow(clippy::too_many_arguments)] // Tauri exposes command fields as individual IPC arguments.
#[tauri::command(async)]
pub fn terminal_open(
    manager: State<'_, TerminalManager>,
    app_state: State<'_, AppState>,
    rows: u16,
    cols: u16,
    host: Option<heminus_domain::Host>,
    session_title: Option<String>,
    cwd: Option<PathBuf>,
    on_event: Channel<InvokeResponseBody>,
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
    let mut connection_log: Option<Vec<crate::ssh_runtime::ConnectionHop>> = None;
    let mut remote_session = false;
    let mut command = if let Some(host) = host {
        host.validate().map_err(|error| error.to_string())?;
        if is_local_host(&host) {
            local_shell_command(&host.environment, cwd.as_deref())?
        } else {
            remote_session = true;
            let (identity, host_arguments, hops, credential_environment) = {
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
                let credential_environment = crate::credential::connection_askpass_environment(
                    &database,
                    &host,
                    identity.as_ref(),
                )?;
                // Forced askpass swallows the host-key question too, so the
                // policy has to depend on whether Heminus is answering prompts.
                let policy = crate::ssh_runtime::HostKeyPolicy::for_forced_askpass(
                    credential_environment.is_some(),
                );
                let (host_arguments, hops) = app_state
                    .ssh
                    .interactive_connection(&database, &host, policy)?;
                (identity, host_arguments, hops, credential_environment)
            };
            let policy = crate::ssh_runtime::HostKeyPolicy::for_forced_askpass(
                credential_environment.is_some(),
            );
            connection_artifacts = app_state
                .ssh
                .connection_artifacts(&host_arguments)
                .with_logs(hops.iter().map(|hop| hop.log.clone()));
            // The last hop is the host itself, so its log belongs on this
            // process; the inner hops carry theirs inside the chain config.
            connection_log = Some(hops.clone());
            let username = identity
                .as_ref()
                .and_then(|value| value.username.as_deref())
                .unwrap_or(&host.username);
            let mut ssh = CommandBuilder::new(crate::platform::ssh_executable()?);
            for argument in app_state.ssh.interactive_arguments(policy) {
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
            if let Some(hop) = connection_log.as_ref().and_then(|hops| hops.last()) {
                // -E keeps the handshake chatter out of the terminal; the app
                // streams it into the connecting screen instead. LogLevel is
                // used rather than -v so the verbosity is not inherited by the
                // chain's nested SSH processes, which log to their own files.
                ssh.arg("-o");
                ssh.arg("LogLevel=DEBUG1");
                ssh.arg("-E");
                ssh.arg(&hop.log);
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
        local_shell_command(&[], cwd.as_deref())?
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
        destination: Some(on_event),
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
                writer: Arc::new(Mutex::new(writer)),
                killer: session_killer,
                remote: remote_session,
                history_id,
                event_sink: Arc::clone(&event_sink),
                supervisor,
                _artifacts: connection_artifacts,
            },
        );

    let log_finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Some(hops) = connection_log {
        publish_terminal_event(
            &event_sink,
            TerminalEvent::Hops {
                labels: hops.iter().map(|hop| hop.label.clone()).collect(),
            },
        );
        for (index, hop) in hops.into_iter().enumerate() {
            spawn_connection_log_reader(
                index,
                hop.log,
                Arc::clone(&event_sink),
                Arc::clone(&log_finished),
            );
        }
    }

    let spawned = thread::Builder::new()
        .name(format!("heminus-pty-{id}"))
        .spawn({
            let log_finished = Arc::clone(&log_finished);
            let sessions = Arc::clone(&sessions);
            let reader_database = Arc::clone(&manager.database);
            move || {
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
                log_finished.store(true, std::sync::atomic::Ordering::Relaxed);
                finish_session_quietly(&reader_database, history_id);
                if let Ok(mut sessions) = sessions.lock() {
                    sessions.remove(&id);
                }
                publish_terminal_event(&event_sink, TerminalEvent::Exit);
            }
        });
    if let Err(error) = spawned {
        // The reader never started, so nothing would ever stop the log
        // readers, finish the history row, or drop the session's artifacts.
        log_finished.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut sessions) = sessions.lock()
            && let Some(mut session) = sessions.remove(&id)
        {
            let _ = session.killer.kill();
        }
        finish_session_quietly(&manager.database, history_id);
        return Err(error.to_string());
    }

    Ok(id)
}

#[tauri::command(async)]
pub fn terminal_attach(
    manager: State<'_, TerminalManager>,
    id: Uuid,
    on_event: Channel<InvokeResponseBody>,
) -> Result<bool, String> {
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
    // The backlog is replayed while the sink lock is held. Releasing it first
    // would let the reader thread push live output to the newly attached window
    // ahead of the backlog, so the pane would show the newest bytes and then
    // repeat them inside the replay.
    let previous_destination = {
        let mut sink = event_sink
            .lock()
            .map_err(|_| "terminal event sink lock poisoned")?;
        sink.generation = sink.generation.wrapping_add(1);
        let previous_destination = sink.destination.replace(on_event.clone());
        // Sent in pieces: the buffer holds up to 2 MiB, and one message that
        // size stalls the webview on arrival.
        let replay = sink.replay.iter().copied().collect::<Vec<_>>();
        for chunk in replay.chunks(TERMINAL_REPLAY_CHUNK) {
            if on_event
                .send(InvokeResponseBody::Raw(chunk.to_vec()))
                .is_err()
            {
                sink.destination = None;
                return Err("Could not attach the detached terminal output".to_string());
            }
        }
        previous_destination
    };
    drop(previous_destination);

    Ok(true)
}

/// Stops streaming to the current listener without ending the session.
///
/// A pane torn down for a move keeps its session alive; without this the
/// backend would keep pushing output at a webview that is no longer listening
/// until a send finally failed.
#[tauri::command(async)]
pub fn terminal_detach(manager: State<'_, TerminalManager>, id: Uuid) -> Result<bool, String> {
    let sessions = manager
        .sessions
        .lock()
        .map_err(|_| "terminal lock poisoned")?;
    let Some(session) = sessions.get(&id) else {
        return Ok(false);
    };
    let mut sink = session
        .event_sink
        .lock()
        .map_err(|_| "terminal event sink lock poisoned")?;
    sink.generation = sink.generation.wrapping_add(1);
    sink.destination = None;
    Ok(true)
}

/// Writes input to a session's PTY.
///
/// The write has to happen off both the UI thread and the session map's lock: a
/// child that has stopped reading (Ctrl+S, or a stalled program) fills the
/// kernel buffer and blocks the write indefinitely. Holding the map lock there
/// froze every other terminal along with the whole window.
#[tauri::command]
pub async fn terminal_write(
    manager: State<'_, TerminalManager>,
    id: Uuid,
    bytes: Vec<u8>,
) -> Result<(), String> {
    let writer = {
        let sessions = manager
            .sessions
            .lock()
            .map_err(|_| "terminal lock poisoned")?;
        Arc::clone(
            &sessions
                .get(&id)
                .ok_or_else(|| "terminal session not found".to_string())?
                .writer,
        )
    };
    tauri::async_runtime::spawn_blocking(move || {
        let mut writer = writer
            .lock()
            .map_err(|_| "terminal writer lock poisoned".to_string())?;
        writer
            .write_all(&bytes)
            .and_then(|_| writer.flush())
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
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

/// Lists the processes a terminal session started and would leave behind.
///
/// SSH tabs report nothing: every local process under them is Heminus's own
/// transport — the client, its jump-host hops, the proxy connector — and the
/// work the person actually started lives on the server, out of reach.
#[tauri::command(async)]
pub fn terminal_processes(
    manager: State<'_, TerminalManager>,
    id: Uuid,
) -> Result<Vec<crate::platform::SessionProcess>, String> {
    let sessions = manager
        .sessions
        .lock()
        .map_err(|_| "terminal lock poisoned")?;
    let Some(session) = sessions.get(&id).filter(|session| !session.remote) else {
        return Ok(Vec::new());
    };
    Ok(session.supervisor.background_processes())
}

/// Stops the chosen processes without closing the terminal itself.
#[tauri::command(async)]
pub fn terminal_kill_processes(
    manager: State<'_, TerminalManager>,
    id: Uuid,
    pids: Vec<u32>,
) -> Result<Vec<crate::platform::SessionProcess>, String> {
    let supervisor_result = {
        let sessions = manager
            .sessions
            .lock()
            .map_err(|_| "terminal lock poisoned")?;
        let session = sessions
            .get(&id)
            .filter(|session| !session.remote)
            .ok_or_else(|| "terminal session not found".to_string())?;
        session.supervisor.terminate_processes(&pids)?;
        session.supervisor.background_processes()
    };
    Ok(supervisor_result)
}

#[tauri::command(async)]
pub fn terminal_close(
    manager: State<'_, TerminalManager>,
    app_state: State<'_, AppState>,
    id: Uuid,
    kill_processes: Option<bool>,
) -> Result<bool, String> {
    let removed = manager
        .sessions
        .lock()
        .map_err(|_| "terminal lock poisoned")?
        .remove(&id);
    let Some(mut session) = removed else {
        return Ok(false);
    };
    // Closing a tab kills the whole session by default; opting out leaves the
    // background processes the person chose to keep.
    if kill_processes.unwrap_or(true) {
        session.supervisor.terminate();
    }
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

#[tauri::command(async)]
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
    // Same teardown as closing the tab: killing only the leader left whatever
    // the session had started running with nothing supervising it.
    session.supervisor.terminate();
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

#[tauri::command(async)]
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

    /// A sink wired to a channel that records everything it is handed.
    fn recording_sink() -> (
        Arc<Mutex<TerminalEventSink>>,
        Arc<Mutex<Vec<InvokeResponseBody>>>,
    ) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&sent);
        let channel = Channel::new(move |body| {
            captured.lock().unwrap().push(body);
            Ok(())
        });
        let sink = Arc::new(Mutex::new(TerminalEventSink {
            destination: Some(channel),
            replay: VecDeque::new(),
            generation: 0,
        }));
        (sink, sent)
    }

    fn raw_bodies(sent: &Arc<Mutex<Vec<InvokeResponseBody>>>) -> Vec<Vec<u8>> {
        sent.lock()
            .unwrap()
            .iter()
            .filter_map(|body| match body {
                InvokeResponseBody::Raw(bytes) => Some(bytes.clone()),
                InvokeResponseBody::Json(_) => None,
            })
            .collect()
    }

    #[test]
    fn output_is_sent_as_raw_bytes_and_kept_for_replay() {
        let (sink, sent) = recording_sink();

        publish_terminal_output(&sink, b"first ");
        publish_terminal_output(&sink, b"second");

        // Raw keeps a 16 KiB read at 16 KiB instead of ~75 KB of JSON digits.
        assert_eq!(
            raw_bodies(&sent),
            vec![b"first ".to_vec(), b"second".to_vec()]
        );
        let replay = sink
            .lock()
            .unwrap()
            .replay
            .iter()
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(replay, b"first second");
    }

    #[test]
    fn control_events_are_sent_as_json_on_the_same_channel() {
        let (sink, sent) = recording_sink();

        publish_terminal_output(&sink, b"bye");
        publish_terminal_event(&sink, TerminalEvent::Exit);

        let bodies = sent.lock().unwrap();
        assert!(matches!(bodies[0], InvokeResponseBody::Raw(_)));
        // Ordering matters: Exit must not overtake the output before it.
        match &bodies[1] {
            InvokeResponseBody::Json(json) => assert!(json.contains("\"kind\":\"exit\""), "{json}"),
            other => panic!("expected JSON control event, got {other:?}"),
        }
    }

    #[test]
    fn the_replay_buffer_keeps_only_the_newest_bytes() {
        let (sink, _sent) = recording_sink();

        publish_terminal_output(&sink, &vec![b'o'; TERMINAL_REPLAY_LIMIT]);
        publish_terminal_output(&sink, b"tail");

        let replay = sink
            .lock()
            .unwrap()
            .replay
            .iter()
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(replay.len(), TERMINAL_REPLAY_LIMIT);
        assert!(replay.ends_with(b"tail"));
    }

    #[test]
    fn live_output_cannot_overtake_the_backlog_while_a_pane_re_attaches() {
        let (sink, _first) = recording_sink();
        publish_terminal_output(&sink, b"old output ");

        // A second window attaches while the PTY keeps producing. The reader
        // thread blocks on the sink lock until the backlog has gone out, so the
        // new bytes must land after it rather than in front of it.
        let sent = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&sent);
        let reattached = Channel::new(move |body| {
            captured.lock().unwrap().push(body);
            Ok(())
        });
        {
            let mut guard = sink.lock().unwrap();
            guard.generation = guard.generation.wrapping_add(1);
            guard.destination = Some(reattached.clone());
            let replay = guard.replay.iter().copied().collect::<Vec<_>>();
            for chunk in replay.chunks(TERMINAL_REPLAY_CHUNK) {
                reattached
                    .send(InvokeResponseBody::Raw(chunk.to_vec()))
                    .unwrap();
            }
        }
        publish_terminal_output(&sink, b"new output");

        let bodies = raw_bodies(&sent);
        assert_eq!(
            bodies,
            vec![b"old output ".to_vec(), b"new output".to_vec()]
        );
    }

    #[test]
    fn a_failed_send_detaches_only_the_destination_that_failed() {
        let failing = Channel::new(|_body| Err(tauri::Error::WebviewNotFound));
        let sink = Arc::new(Mutex::new(TerminalEventSink {
            destination: Some(failing),
            replay: VecDeque::new(),
            generation: 0,
        }));

        publish_terminal_output(&sink, b"gone");
        assert!(sink.lock().unwrap().destination.is_none());

        // A stale failure must not tear down a destination installed since.
        let (replacement, sent) = recording_sink();
        let replacement = replacement.lock().unwrap().destination.clone();
        {
            let mut guard = sink.lock().unwrap();
            guard.generation = guard.generation.wrapping_add(1);
            guard.destination = replacement;
        }
        clear_failed_destination(&sink, 0);
        assert!(
            sink.lock().unwrap().destination.is_some(),
            "a send that failed before the re-attach must not detach the new listener"
        );

        publish_terminal_output(&sink, b"still here");
        assert_eq!(raw_bodies(&sent), vec![b"still here".to_vec()]);
    }

    #[test]
    fn loopback_hosts_use_a_local_shell_with_their_profile_environment() {
        let mut host = Host::new("Local profile", "127.0.0.1", "local");
        host.environment.push(EnvironmentVariable {
            name: "hi".into(),
            value: "ha".into(),
        });

        assert!(is_local_host(&host));
        let command = local_shell_command(&host.environment, None).unwrap();
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
    fn local_shells_start_in_the_requested_directory_when_it_exists() {
        let requested = std::env::temp_dir();
        let command = local_shell_command(&[], Some(&requested)).unwrap();
        assert_eq!(command.get_cwd(), Some(&requested.into_os_string()));

        let missing = Path::new("/heminus-does-not-exist");
        let fallback = local_shell_command(&[], Some(missing)).unwrap();
        assert_ne!(
            fallback.get_cwd(),
            Some(&missing.as_os_str().to_os_string())
        );
        assert!(fallback.get_cwd().is_some());
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
    fn connection_log_lines_drive_the_connecting_screen() {
        let stage = |line: &str| classify_connection_log(line).and_then(|entry| entry.2);
        assert_eq!(
            stage("debug1: Connecting to 10.0.0.5 [10.0.0.5] port 22."),
            Some(ConnectionStage::Connecting)
        );
        assert_eq!(
            stage("debug1: Connection established."),
            Some(ConnectionStage::Handshake)
        );
        assert_eq!(
            stage("debug1: Authenticating to 10.0.0.5:22 as 'deploy'"),
            Some(ConnectionStage::Authenticating)
        );
        assert_eq!(
            stage("debug1: Authentication succeeded (password)."),
            Some(ConnectionStage::Authenticated)
        );
        assert_eq!(
            stage("debug1: Entering interactive session."),
            Some(ConnectionStage::Ready)
        );
        assert_eq!(stage("debug1: Reading configuration data none"), None);
        // A tidy logout is not a failure.
        assert_eq!(stage("Connection to 10.0.0.5 closed."), None);
    }

    #[test]
    fn connection_log_hides_heminus_plumbing() {
        for line in [
            "debug1: Reading configuration data /home/x/.local/share/Heminus/ssh/connections/a.conf",
            "debug1: Executing proxy command: exec \"/usr/bin/ssh\" -F /tmp/a.conf -W \"[10.0.0.5]:22\" heminus-jump-1",
            "debug1: Applying options for heminus-jump-1",
        ] {
            assert!(
                classify_connection_log(line).is_none(),
                "internal plumbing should stay out of the log: {line}"
            );
        }
        assert!(classify_connection_log("debug1: Connection established.").is_some());
    }

    #[test]
    fn connection_log_failures_are_reported_as_errors() {
        for line in [
            "ssh: connect to host 10.0.0.5 port 22: Connection refused",
            "deploy@10.0.0.5: Permission denied (publickey,password).",
            "Host key verification failed.",
            "heminus proxy: The proxy rejected the credentials (407 Proxy Authentication Required)",
            "kex_exchange_identification: Connection closed by remote host",
            "Timeout, server 10.0.0.5 not responding.",
            "Unable to negotiate with 10.0.0.5 port 22: no matching host key type found",
        ] {
            let (level, _, stage) = classify_connection_log(line).expect("a failure entry");
            assert_eq!(level, LogLevel::Error, "{line}");
            assert_eq!(stage, Some(ConnectionStage::Failed), "{line}");
        }
    }

    #[test]
    fn connection_log_separates_debug_chatter_from_plain_messages() {
        let (level, message, _) =
            classify_connection_log("debug1: Local version string SSH-2.0-OpenSSH_9.6").unwrap();
        assert_eq!(level, LogLevel::Debug);
        assert_eq!(message, "Local version string SSH-2.0-OpenSSH_9.6");

        let (level, message, _) = classify_connection_log(
            "Warning: Permanently added '10.0.0.5' to the list of known hosts.",
        )
        .unwrap();
        assert_eq!(level, LogLevel::Info, "a pinned key is news, not a warning");
        assert!(message.contains("Permanently added"));

        assert!(classify_connection_log("   ").is_none());
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
