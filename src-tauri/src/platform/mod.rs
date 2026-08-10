use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use directories::UserDirs;
use serde::Serialize;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as implementation;
#[cfg(windows)]
use windows as implementation;

#[derive(Clone, Debug)]
pub struct LocalShell {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
}

pub struct ProcessSupervisor {
    inner: implementation::ProcessSupervisor,
}

impl ProcessSupervisor {
    pub fn attach(pid: Option<u32>) -> Result<Self, String> {
        implementation::ProcessSupervisor::attach(pid).map(|inner| Self { inner })
    }

    pub fn terminate(&self) {
        self.inner.terminate();
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub local_permission_editing: bool,
    pub custom_application_open: bool,
    pub foreign_process_termination: bool,
    pub ssh_available: bool,
    pub ssh_keygen_available: bool,
    pub local_shell: Option<String>,
}

pub fn capabilities() -> PlatformCapabilities {
    PlatformCapabilities {
        local_permission_editing: cfg!(unix),
        custom_application_open: cfg!(unix),
        foreign_process_termination: cfg!(unix),
        ssh_available: ssh_executable().is_ok(),
        ssh_keygen_available: ssh_keygen_executable().is_ok(),
        local_shell: local_shell().ok().and_then(|shell| {
            shell
                .executable
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        }),
    }
}

pub fn home_dir() -> Result<PathBuf, String> {
    UserDirs::new()
        .map(|directories| directories.home_dir().to_path_buf())
        .filter(|path| path.is_absolute())
        .ok_or_else(|| "Could not resolve the user profile directory".to_string())
}

pub fn canonical_home_dir() -> Result<PathBuf, String> {
    canonicalize(&home_dir()?)
}

pub fn canonical_existing_local_path(path: &Path) -> Result<PathBuf, String> {
    let canonical = canonicalize(path)?;
    ensure_inside_home(&canonical)?;
    Ok(canonical)
}

pub fn safe_existing_local_entry(path: &Path) -> Result<PathBuf, String> {
    let file_name = valid_file_name(path, "Choose a valid file or folder")?;
    let parent = path
        .parent()
        .ok_or_else(|| "Choose a valid parent directory".to_string())?;
    let canonical_parent = canonical_existing_local_path(parent)?;
    let target = canonical_parent.join(file_name);
    fs::symlink_metadata(&target).map_err(|error| error.to_string())?;
    let resolved_target = canonicalize(&target)?;
    ensure_inside_home(&resolved_target)?;
    Ok(target)
}

pub fn safe_local_destination(path: &Path) -> Result<PathBuf, String> {
    let file_name = valid_file_name(path, "Choose a valid file or folder name")?;
    let parent = path
        .parent()
        .ok_or_else(|| "Choose a destination directory".to_string())?;
    Ok(canonical_existing_local_path(parent)?.join(file_name))
}

pub fn join_local_path(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || Path::new(name).components().count() != 1
        || Path::new(name).file_name() != Some(OsStr::new(name))
    {
        return Err("Choose a file or folder name without path separators".into());
    }
    Ok(canonical_existing_local_path(parent)?.join(name))
}

pub fn local_shell() -> Result<LocalShell, String> {
    implementation::local_shell()
}

pub fn ssh_executable() -> Result<PathBuf, String> {
    implementation::ssh_tool("ssh")
}

pub fn ssh_keygen_executable() -> Result<PathBuf, String> {
    implementation::ssh_tool("ssh-keygen")
}

pub fn secure_private_directory(path: &Path) -> Result<(), String> {
    implementation::secure_private_directory(path)
}

pub fn secure_private_file(path: &Path) -> Result<(), String> {
    implementation::secure_private_file(path)
}

pub fn configure_background_command(command: &mut std::process::Command) {
    implementation::configure_background_command(command)
}

pub fn primary_pointer_pressed() -> bool {
    implementation::primary_pointer_pressed()
}

pub fn open_path(path: &Path, application: Option<&str>) -> Result<(), String> {
    let result = match application.map(str::trim).filter(|value| !value.is_empty()) {
        Some(application) if cfg!(windows) => {
            return Err(format!(
                "Choosing a custom application is not supported on Windows yet: {application}"
            ));
        }
        Some(application) => open::with_detached(path, application),
        None => open::that_detached(path),
    };
    result.map_err(|error| format!("Could not open {}: {error}", path.display()))
}

pub(crate) fn executable_from_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

fn canonicalize(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|error| format!("Could not resolve {}: {error}", path.display()))
}

fn ensure_inside_home(path: &Path) -> Result<(), String> {
    let home = canonical_home_dir()?;
    if path.starts_with(&home) {
        Ok(())
    } else {
        Err(format!(
            "Local file access is limited to the user profile: {}",
            home.display()
        ))
    }
}

fn valid_file_name<'a>(path: &'a Path, message: &str) -> Result<&'a OsStr, String> {
    let file_name = path.file_name().ok_or_else(|| message.to_string())?;
    if file_name == "." || file_name == ".." {
        Err(message.into())
    } else {
        Ok(file_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_join_rejects_path_components() {
        let home = home_dir().expect("home directory");
        assert!(join_local_path(&home, "../outside").is_err());
        assert!(join_local_path(&home, "folder/name").is_err());
        assert!(join_local_path(&home, "folder\\name").is_err());
    }

    #[test]
    #[cfg(windows)]
    fn windows_supervisor_terminates_its_process() {
        let shell = std::env::var_os("ComSpec").expect("ComSpec");
        let mut child = std::process::Command::new(shell)
            .args(["/C", "ping -n 30 127.0.0.1 >nul"])
            .spawn()
            .expect("test process");
        let supervisor = ProcessSupervisor::attach(Some(child.id())).expect("job object");
        supervisor.terminate();
        assert!(!child.wait().expect("terminated process").success());
    }
}
