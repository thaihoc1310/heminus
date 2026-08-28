use std::collections::HashMap;
#[cfg(target_family = "unix")]
use std::fs;
use std::io::Read;
#[cfg(target_family = "unix")]
use std::os::unix::fs::MetadataExt;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
#[cfg(target_family = "unix")]
use std::thread;
#[cfg(target_family = "unix")]
use std::time::Duration;

use heminus_domain::{ForwardKind, Host, IdentityKind, PortForward};
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::AppState;

struct ActiveTunnel {
    child: Child,
    supervisor: crate::platform::ProcessSupervisor,
    _artifacts: crate::ssh_runtime::ConnectionArtifacts,
}

#[derive(Default)]
pub struct TunnelManager {
    active: Mutex<HashMap<Uuid, ActiveTunnel>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelState {
    id: Uuid,
    running: bool,
    exit_code: Option<i32>,
    message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelPortProcess {
    pid: u32,
    name: String,
    command: String,
    requires_elevation: bool,
}

impl TunnelManager {
    fn reap(&self) -> Result<Vec<TunnelState>, String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "tunnel lock poisoned".to_string())?;
        let mut states = Vec::with_capacity(active.len());
        let mut completed = Vec::new();
        for (id, tunnel) in active.iter_mut() {
            match tunnel.child.try_wait().map_err(|error| error.to_string())? {
                Some(status) => {
                    let message = tunnel.child.stderr.take().and_then(|stderr| {
                        let mut bytes = Vec::new();
                        stderr.take(8 * 1024).read_to_end(&mut bytes).ok()?;
                        normalize_ssh_error(&bytes)
                    });
                    states.push(TunnelState {
                        id: *id,
                        running: false,
                        exit_code: status.code(),
                        message,
                    });
                    completed.push(*id);
                }
                None => states.push(TunnelState {
                    id: *id,
                    running: true,
                    exit_code: None,
                    message: None,
                }),
            }
        }
        for id in completed {
            active.remove(&id);
        }
        Ok(states)
    }
}

impl Drop for TunnelManager {
    fn drop(&mut self) {
        if let Ok(active) = self.active.get_mut() {
            for tunnel in active.values_mut() {
                tunnel.supervisor.terminate();
                let _ = tunnel.child.kill();
                let _ = tunnel.child.wait();
            }
        }
    }
}

#[tauri::command]
pub fn tunnel_start(
    manager: State<'_, TunnelManager>,
    app_state: State<'_, AppState>,
    rule: PortForward,
    host: Host,
) -> Result<(), String> {
    rule.validate().map_err(|error| error.to_string())?;
    host.validate().map_err(|error| error.to_string())?;
    if rule.host_id != host.id {
        return Err("The forwarding rule does not belong to this host".into());
    }
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
        let credential_environment =
            crate::credential::connection_askpass_environment(&database, &host, identity.as_ref())?;
        // A background tunnel has no terminal at all, and forced askpass would
        // swallow the host-key question, so pin unknown keys on first use.
        let policy =
            crate::ssh_runtime::HostKeyPolicy::for_forced_askpass(credential_environment.is_some());
        let host_arguments = app_state.ssh.host_arguments(&database, &host, policy)?;
        (identity, host_arguments, credential_environment)
    };
    let host_key_policy =
        crate::ssh_runtime::HostKeyPolicy::for_forced_askpass(credential_environment.is_some());
    let username = identity
        .as_ref()
        .and_then(|value| value.username.as_deref())
        .unwrap_or(&host.username);
    let connection_artifacts = app_state.ssh.connection_artifacts(&host_arguments);

    let forwarding_argument = match rule.kind {
        ForwardKind::Local => format!(
            "{}:{}:{}:{}",
            rule.bind_host,
            rule.bind_port,
            rule.destination_host.as_deref().unwrap_or_default(),
            rule.destination_port.unwrap_or_default()
        ),
        ForwardKind::Remote => format!(
            "{}:{}:{}:{}",
            rule.bind_host,
            rule.bind_port,
            rule.destination_host.as_deref().unwrap_or_default(),
            rule.destination_port.unwrap_or_default()
        ),
        ForwardKind::Dynamic => format!("{}:{}", rule.bind_host, rule.bind_port),
    };
    let forwarding_flag = match rule.kind {
        ForwardKind::Local => "-L",
        ForwardKind::Remote => "-R",
        ForwardKind::Dynamic => "-D",
    };

    let mut active = manager
        .active
        .lock()
        .map_err(|_| "tunnel lock poisoned".to_string())?;
    if let Some(existing) = active.get_mut(&rule.id) {
        if existing
            .child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_none()
        {
            return Ok(());
        }
        active.remove(&rule.id);
    }

    let mut command = Command::new(crate::platform::ssh_executable()?);
    crate::platform::configure_background_command(&mut command);
    let password_identity = identity
        .as_ref()
        .filter(|value| value.kind == IdentityKind::Password);
    for argument in app_state.ssh.interactive_arguments(host_key_policy) {
        command.arg(argument);
    }
    for argument in host_arguments {
        command.arg(argument);
    }
    command
        .arg("-N")
        .arg("-T")
        .env("LC_ALL", "C")
        .env_remove("SSH_AUTH_SOCK")
        .env_remove("SSH_ASKPASS")
        .env_remove("SSH_ASKPASS_REQUIRE")
        .env_remove(crate::credential::askpass_candidates_env())
        .arg("-o")
        .arg(
            if password_identity.is_some() || credential_environment.is_some() {
                "BatchMode=no"
            } else {
                "BatchMode=yes"
            },
        )
        .arg("-o")
        .arg("ExitOnForwardFailure=yes")
        .arg("-o")
        .arg("ServerAliveInterval=30")
        .arg("-o")
        .arg("ServerAliveCountMax=3")
        .arg("-p")
        .arg(host.port.to_string())
        .arg(forwarding_flag)
        .arg(forwarding_argument);
    if let Some(key_path) = identity
        .as_ref()
        .filter(|value| value.kind == IdentityKind::KeyFile)
        .and_then(|value| value.key_path.as_deref())
    {
        command
            .arg("-o")
            .arg("IdentitiesOnly=yes")
            .arg("-i")
            .arg(key_path);
    }
    if let Some(identity) = password_identity {
        if !identity.secret_stored {
            return Err("This password identity does not have a stored password".into());
        }
        command
            .arg("-o")
            .arg("PreferredAuthentications=keyboard-interactive,password")
            .arg("-o")
            .arg("PubkeyAuthentication=no")
            .arg("-o")
            .arg("NumberOfPasswordPrompts=1");
    }
    if let Some((askpass, candidates)) = credential_environment {
        command
            .env("SSH_ASKPASS", askpass)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env(crate::credential::askpass_candidates_env(), candidates);
        #[cfg(unix)]
        if std::env::var_os("DISPLAY").is_none() {
            command.env("DISPLAY", "heminus:0");
        }
    }
    let mut child = command
        .arg("--")
        .arg(format!(
            "{username}@{}",
            crate::ssh_runtime::effective_address(&host)
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start SSH forwarding: {error}"))?;
    let supervisor = match crate::platform::ProcessSupervisor::attach(Some(child.id())) {
        Ok(supervisor) => supervisor,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    active.insert(
        rule.id,
        ActiveTunnel {
            child,
            supervisor,
            _artifacts: connection_artifacts,
        },
    );
    Ok(())
}

#[tauri::command]
pub fn tunnel_stop(manager: State<'_, TunnelManager>, id: Uuid) -> Result<bool, String> {
    let mut active = manager
        .active
        .lock()
        .map_err(|_| "tunnel lock poisoned".to_string())?;
    let Some(mut tunnel) = active.remove(&id) else {
        return Ok(false);
    };
    tunnel.supervisor.terminate();
    tunnel.child.kill().map_err(|error| error.to_string())?;
    let _ = tunnel.child.wait();
    Ok(true)
}

#[tauri::command]
pub fn tunnel_states(manager: State<'_, TunnelManager>) -> Result<Vec<TunnelState>, String> {
    manager.reap()
}

#[tauri::command]
pub fn tunnel_port_processes(port: u16) -> Result<Vec<TunnelPortProcess>, String> {
    let own_pid = std::process::id();
    let own_uid = process_uid(own_pid);
    Ok(listener_pids(port)?
        .into_iter()
        .filter(|pid| *pid != own_pid)
        .map(|pid| TunnelPortProcess {
            pid,
            name: process_name(pid),
            command: process_command(pid),
            requires_elevation: process_uid(pid)
                .zip(own_uid)
                .is_none_or(|(uid, own)| uid != own),
        })
        .collect())
}

#[tauri::command]
#[cfg(target_family = "unix")]
pub fn tunnel_stop_port_processes(port: u16, pids: Vec<u32>) -> Result<(), String> {
    let own_pid = std::process::id();
    let mut current = listener_pids(port)?;
    current.retain(|pid| *pid != own_pid);
    let mut targets = pids;
    targets.sort_unstable();
    targets.dedup();
    if targets.iter().any(|pid| !current.contains(pid)) {
        return Err("The process using this port changed. Try starting the tunnel again.".into());
    }

    let status = if targets.is_empty() {
        Command::new("pkexec")
            .arg("/usr/bin/fuser")
            .args(["-k", "-TERM", "-n", "tcp"])
            .arg(port.to_string())
            .status()
            .map_err(|error| format!("Could not open the administrator prompt: {error}"))?
    } else {
        let own_uid = process_uid(own_pid);
        let needs_elevation = targets.iter().any(|pid| {
            process_uid(*pid)
                .zip(own_uid)
                .is_none_or(|(uid, own)| uid != own)
        });
        let mut command = if needs_elevation {
            let mut command = Command::new("pkexec");
            command.arg("/usr/bin/kill");
            command
        } else {
            Command::new("/usr/bin/kill")
        };
        command.arg("-TERM").arg("--");
        for pid in &targets {
            command.arg(pid.to_string());
        }
        command
            .status()
            .map_err(|error| format!("Could not stop the process using port {port}: {error}"))?
    };
    if !status.success() {
        return Err(
            if status.code() == Some(126) || status.code() == Some(127) {
                "Administrator authentication was cancelled or denied.".into()
            } else {
                format!("The process using port {port} could not be stopped.")
            },
        );
    }

    for _ in 0..20 {
        thread::sleep(Duration::from_millis(100));
        let remaining = listener_pids(port)?;
        let target_still_listening = if targets.is_empty() {
            !remaining.is_empty()
        } else {
            targets.iter().any(|pid| remaining.contains(pid))
        };
        if !target_still_listening {
            return Ok(());
        }
    }
    Err(format!(
        "The process did not release port {port} after receiving the stop request."
    ))
}

#[tauri::command]
#[cfg(windows)]
pub fn tunnel_stop_port_processes(port: u16, pids: Vec<u32>) -> Result<(), String> {
    let own_pid = std::process::id();
    let mut current = listener_pids(port)?;
    current.retain(|pid| *pid != own_pid);
    let mut targets = pids;
    targets.sort_unstable();
    targets.dedup();
    if targets.iter().any(|pid| !current.contains(pid)) {
        return Err("The process using this port changed. Try starting the tunnel again.".into());
    }
    Err(
        "Stopping another application's process is not supported on Windows. Close it from Task Manager and try again."
            .into(),
    )
}

#[cfg(target_family = "unix")]
fn listener_pids(port: u16) -> Result<Vec<u32>, String> {
    let output = Command::new("fuser")
        .args(["-n", "tcp"])
        .arg(port.to_string())
        .output()
        .map_err(|error| format!("Could not inspect port {port}: {error}"))?;
    if !output.status.success() && output.status.code() != Some(1) {
        return Err(format!("Could not inspect port {port}."));
    }
    let mut pids = parse_fuser_pids(&output.stdout);
    pids.extend(parse_fuser_pids(&output.stderr));
    pids.sort_unstable();
    pids.dedup();
    Ok(pids)
}

#[cfg(windows)]
fn listener_pids(port: u16) -> Result<Vec<u32>, String> {
    use netstat2::{AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};

    let sockets = netstat2::get_sockets_info(
        AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
        ProtocolFlags::TCP,
    )
    .map_err(|error| format!("Could not inspect port {port}: {error}"))?;
    let mut pids = sockets
        .into_iter()
        .filter(|socket| {
            matches!(
                socket.protocol_socket_info,
                ProtocolSocketInfo::Tcp(ref tcp)
                    if tcp.local_port == port && tcp.state == TcpState::Listen
            )
        })
        .flat_map(|socket| socket.associated_pids)
        .collect::<Vec<_>>();
    pids.sort_unstable();
    pids.dedup();
    Ok(pids)
}

#[cfg(target_family = "unix")]
fn parse_fuser_pids(bytes: &[u8]) -> Vec<u32> {
    String::from_utf8_lossy(bytes)
        .split_whitespace()
        .filter_map(|token| token.parse::<u32>().ok())
        .collect()
}

#[cfg(target_family = "unix")]
fn process_name(pid: u32) -> String {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|name| name.trim().to_string())
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("Process {pid}"))
}

#[cfg(windows)]
fn process_name(pid: u32) -> String {
    let system = sysinfo::System::new_all();
    system
        .process(sysinfo::Pid::from_u32(pid))
        .map(|process| process.name().to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("Process {pid}"))
}

#[cfg(target_family = "unix")]
fn process_command(pid: u32) -> String {
    fs::read(format!("/proc/{pid}/cmdline"))
        .ok()
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .replace('\0', " ")
                .trim()
                .chars()
                .take(240)
                .collect::<String>()
        })
        .filter(|command| !command.is_empty())
        .unwrap_or_else(|| process_name(pid))
}

#[cfg(windows)]
fn process_command(pid: u32) -> String {
    let system = sysinfo::System::new_all();
    let Some(process) = system.process(sysinfo::Pid::from_u32(pid)) else {
        return process_name(pid);
    };
    let command = process
        .cmd()
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    if command.is_empty() {
        process
            .exe()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| process_name(pid))
    } else {
        command.chars().take(240).collect()
    }
}

#[cfg(target_family = "unix")]
fn process_uid(pid: u32) -> Option<u32> {
    fs::metadata(format!("/proc/{pid}"))
        .ok()
        .map(|metadata| metadata.uid())
}

#[cfg(not(target_family = "unix"))]
fn process_uid(_pid: u32) -> Option<u32> {
    None
}

fn normalize_ssh_error(bytes: &[u8]) -> Option<String> {
    let message = String::from_utf8_lossy(bytes)
        .replace(['\r', '\n'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!message.is_empty()).then_some(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_errors_are_compact_for_the_inspector() {
        let message = normalize_ssh_error(b"bind [127.0.0.1]:8080: Address already in use\r\n")
            .expect("message");
        assert_eq!(message, "bind [127.0.0.1]:8080: Address already in use");
    }

    #[test]
    fn empty_ssh_errors_are_omitted() {
        assert_eq!(normalize_ssh_error(b" \n\r "), None);
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn fuser_pid_output_ignores_the_port_label() {
        assert_eq!(
            parse_fuser_pids(b"6443/tcp:  14714 20500\n"),
            vec![14714, 20500]
        );
    }
}
