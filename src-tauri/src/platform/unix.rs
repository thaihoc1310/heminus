use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(not(target_os = "linux"))]
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use super::{LocalShell, SessionProcess, executable_from_path};

/// Watches every process a terminal session started.
///
/// `portable_pty` puts the child in a fresh session via `setsid`, so the child
/// PID doubles as the session ID. Killing only that PID (what Heminus used to
/// do) leaves `npm run dev`, `docker compose up`, and anything else the shell
/// spawned running after the tab is gone, so the supervisor works on the whole
/// session instead.
pub struct ProcessSupervisor {
    session: Option<u32>,
}

impl ProcessSupervisor {
    pub fn attach(pid: Option<u32>) -> Result<Self, String> {
        Ok(Self { session: pid })
    }

    pub fn processes(&self) -> Vec<SessionProcess> {
        let Some(session) = self.session else {
            return Vec::new();
        };
        let own = std::process::id();
        let mut processes = session_processes(session)
            .into_iter()
            .filter(|process| process.pid != own)
            .collect::<Vec<_>>();
        processes.sort_by_key(|process| (!process.leader, process.pid));
        processes
    }

    pub fn terminate(&self) {
        let pids = self
            .processes()
            .into_iter()
            .map(|process| process.pid)
            .collect::<Vec<_>>();
        signal_and_reap(&pids, libc::SIGHUP);
    }

    pub fn terminate_processes(&self, pids: &[u32]) -> Result<(), String> {
        let session = self.processes();
        let unknown = pids
            .iter()
            .find(|pid| !session.iter().any(|process| process.pid == **pid));
        if let Some(pid) = unknown {
            return Err(format!(
                "Process {pid} is no longer part of this terminal session"
            ));
        }
        signal_and_reap(pids, libc::SIGTERM);
        Ok(())
    }
}

/// Sends `signal`, gives the processes a moment to exit, then forces the rest.
fn signal_and_reap(pids: &[u32], signal: i32) {
    let own = std::process::id();
    let targets = pids
        .iter()
        .copied()
        .filter(|pid| *pid > 1 && *pid != own)
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return;
    }
    for pid in &targets {
        send_signal(*pid, signal);
    }
    let deadline = Instant::now() + Duration::from_millis(1_500);
    while Instant::now() < deadline {
        if targets.iter().all(|pid| !is_alive(*pid)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    for pid in targets.iter().filter(|pid| is_alive(**pid)) {
        send_signal(*pid, libc::SIGKILL);
    }
}

fn send_signal(pid: u32, signal: i32) {
    // SAFETY: `kill` only inspects the PID; a stale PID returns ESRCH.
    unsafe {
        libc::kill(pid as libc::pid_t, signal);
    }
}

fn is_alive(pid: u32) -> bool {
    // SAFETY: signal 0 performs the permission and existence check only.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(target_os = "linux")]
fn session_processes(session: u32) -> Vec<SessionProcess> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse::<u32>().ok()?;
            let stat = fs::read_to_string(entry.path().join("stat")).ok()?;
            // The `comm` field is parenthesised and may itself contain spaces
            // and parentheses, so the numeric fields start after the last ')'.
            let tail = &stat[stat.rfind(')')? + 1..];
            let mut fields = tail.split_whitespace();
            let _state = fields.next()?;
            let _parent = fields.next()?;
            let _group = fields.next()?;
            if fields.next()?.parse::<u32>().ok()? != session {
                return None;
            }
            let name = stat[stat.find('(')? + 1..stat.rfind(')')?].to_string();
            let command = fs::read(entry.path().join("cmdline"))
                .ok()
                .map(|raw| {
                    String::from_utf8_lossy(&raw)
                        .split('\0')
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .filter(|value: &String| !value.trim().is_empty())
                .unwrap_or_else(|| format!("[{name}]"));
            Some(SessionProcess {
                pid,
                name,
                command,
                leader: pid == session,
            })
        })
        .collect()
}

/// macOS and the BSDs have no `/proc`, so walk the descendants of the leader.
#[cfg(not(target_os = "linux"))]
fn session_processes(session: u32) -> Vec<SessionProcess> {
    let Ok(output) = Command::new("ps")
        .args(["-Ao", "pid=,ppid=,comm=,args="])
        .output()
    else {
        return Vec::new();
    };
    let mut rows: BTreeMap<u32, (u32, String, String)> = BTreeMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.trim().splitn(4, char::is_whitespace);
        let Some(pid) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let Some(parent) = fields
            .next()
            .and_then(|value| value.trim().parse::<u32>().ok())
        else {
            continue;
        };
        let name = fields
            .next()
            .unwrap_or_default()
            .trim()
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();
        let command = fields.next().unwrap_or_default().trim().to_string();
        rows.insert(pid, (parent, name, command));
    }

    let mut members = vec![session];
    let mut index = 0;
    while index < members.len() {
        let parent = members[index];
        for (pid, (candidate_parent, _, _)) in &rows {
            if *candidate_parent == parent && !members.contains(pid) {
                members.push(*pid);
            }
        }
        index += 1;
    }
    members
        .into_iter()
        .filter_map(|pid| {
            let (_, name, command) = rows.get(&pid)?;
            Some(SessionProcess {
                pid,
                name: name.clone(),
                command: if command.is_empty() {
                    name.clone()
                } else {
                    command.clone()
                },
                leader: pid == session,
            })
        })
        .collect()
}

pub fn local_shell() -> Result<LocalShell, String> {
    let configured = std::env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/bash"));
    let executable = PathBuf::from(&configured);
    let executable = if executable.is_absolute() {
        executable
    } else {
        executable_from_path(&configured.to_string_lossy()).ok_or_else(|| {
            format!(
                "Could not find the configured shell: {}",
                configured.to_string_lossy()
            )
        })?
    };
    Ok(LocalShell {
        executable,
        arguments: vec![OsString::from("-l")],
    })
}

pub fn ssh_tool(name: &str) -> Result<PathBuf, String> {
    executable_from_path(name).ok_or_else(|| {
        format!("Could not find {name}. Install the OpenSSH client and restart Heminus.")
    })
}

pub fn secure_private_directory(path: &Path) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("Could not secure {}: {error}", path.display()))
}

pub fn secure_private_file(path: &Path) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Could not secure {}: {error}", path.display()))
}

pub fn configure_background_command(_command: &mut Command) {}

pub fn primary_pointer_pressed() -> bool {
    true
}
