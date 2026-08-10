use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{LocalShell, executable_from_path};

pub struct ProcessSupervisor;

impl ProcessSupervisor {
    pub fn attach(_pid: Option<u32>) -> Result<Self, String> {
        Ok(Self)
    }

    pub fn terminate(&self) {}
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
