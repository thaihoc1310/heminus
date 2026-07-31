use std::collections::HashMap;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use heminus_domain::{ForwardKind, Host, IdentityKind, PortForward};
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::AppState;

struct ActiveTunnel {
    child: Child,
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
        let host_arguments = app_state.ssh.host_arguments(&database, &host)?;
        let credential_environment =
            crate::credential::connection_askpass_environment(&database, &host, identity.as_ref())?;
        (identity, host_arguments, credential_environment)
    };
    let username = identity
        .as_ref()
        .and_then(|value| value.username.as_deref())
        .unwrap_or(&host.username);

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

    let mut command = Command::new("ssh");
    let password_identity = identity
        .as_ref()
        .filter(|value| value.kind == IdentityKind::Password);
    for argument in app_state.ssh.arguments() {
        command.arg(argument);
    }
    for argument in host_arguments {
        command.arg(argument);
    }
    command
        .arg("-N")
        .arg("-T")
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
        if std::env::var_os("DISPLAY").is_none() {
            command.env("DISPLAY", "heminus:0");
        }
    }
    let child = command
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
    active.insert(rule.id, ActiveTunnel { child });
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
    tunnel.child.kill().map_err(|error| error.to_string())?;
    let _ = tunnel.child.wait();
    Ok(true)
}

#[tauri::command]
pub fn tunnel_states(manager: State<'_, TunnelManager>) -> Result<Vec<TunnelState>, String> {
    manager.reap()
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
}
