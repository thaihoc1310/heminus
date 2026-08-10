use std::ffi::OsString;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};

use super::{LocalShell, executable_from_path};

pub struct ProcessSupervisor {
    job: HANDLE,
}

unsafe impl Send for ProcessSupervisor {}
unsafe impl Sync for ProcessSupervisor {}

impl ProcessSupervisor {
    pub fn attach(pid: Option<u32>) -> Result<Self, String> {
        let pid = pid.ok_or_else(|| "Could not determine the child process ID".to_string())?;
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(format!(
                    "Could not create a Windows process supervisor: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let mut information: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &information as *const _ as *const _,
                std::mem::size_of_val(&information) as u32,
            ) == 0
            {
                let error = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!(
                    "Could not configure the Windows process supervisor: {error}"
                ));
            }
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if process.is_null() {
                let error = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!("Could not supervise child process {pid}: {error}"));
            }
            let assigned = AssignProcessToJobObject(job, process);
            let error = std::io::Error::last_os_error();
            CloseHandle(process);
            if assigned == 0 {
                CloseHandle(job);
                return Err(format!(
                    "Could not assign child process {pid} to its job: {error}"
                ));
            }
            Ok(Self { job })
        }
    }

    pub fn terminate(&self) {
        if !self.job.is_null() {
            unsafe {
                TerminateJobObject(self.job, 1);
            }
        }
    }
}

impl Drop for ProcessSupervisor {
    fn drop(&mut self) {
        if !self.job.is_null() {
            unsafe {
                CloseHandle(self.job);
            }
            self.job = std::ptr::null_mut();
        }
    }
}

pub fn local_shell() -> Result<LocalShell, String> {
    let mut candidates = Vec::new();
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        candidates.push((
            PathBuf::from(program_files).join("PowerShell/7/pwsh.exe"),
            vec![OsString::from("-NoLogo")],
        ));
    }
    if let Some(system_root) = system_root() {
        candidates.push((
            system_root.join("System32/WindowsPowerShell/v1.0/powershell.exe"),
            vec![OsString::from("-NoLogo")],
        ));
    }
    if let Some(command_shell) = std::env::var_os("ComSpec") {
        candidates.push((PathBuf::from(command_shell), Vec::new()));
    }
    candidates
        .into_iter()
        .find(|(path, _)| path.is_file())
        .map(|(executable, arguments)| LocalShell {
            executable,
            arguments,
        })
        .ok_or_else(|| "Could not find PowerShell or cmd.exe".to_string())
}

pub fn ssh_tool(name: &str) -> Result<PathBuf, String> {
    let executable_name = format!("{name}.exe");
    if let Some(system_root) = system_root() {
        let system_tool = system_root.join("System32/OpenSSH").join(&executable_name);
        if system_tool.is_file() {
            return Ok(system_tool);
        }
    }
    executable_from_path(&executable_name).ok_or_else(|| {
        format!(
            "Microsoft OpenSSH Client is not installed ({executable_name} was not found). Install it from Windows Optional Features and restart Heminus."
        )
    })
}

pub fn secure_private_directory(path: &Path) -> Result<(), String> {
    secure_with_icacls(path, "(OI)(CI)F")
}

pub fn secure_private_file(path: &Path) -> Result<(), String> {
    secure_with_icacls(path, "F")
}

pub fn configure_background_command(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

pub fn primary_pointer_pressed() -> bool {
    unsafe { GetAsyncKeyState(VK_LBUTTON as i32) < 0 }
}

fn secure_with_icacls(path: &Path, rights: &str) -> Result<(), String> {
    let account = whoami::account_os()
        .map_err(|error| format!("Could not identify the current Windows account: {error}"))?;
    let account = account.to_string_lossy();
    let icacls = system_root()
        .map(|root| root.join("System32/icacls.exe"))
        .filter(|candidate| candidate.is_file())
        .ok_or_else(|| "Could not find the Windows ACL utility".to_string())?;
    let mut command = Command::new(icacls);
    configure_background_command(&mut command);
    let output = command
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{account}:{rights}"))
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Could not secure {}: {error}", path.display()))?;
    if output.status.success() {
        Ok(())
    } else {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if message.is_empty() {
            format!("Could not secure the Windows ACL for {}", path.display())
        } else {
            format!(
                "Could not secure the Windows ACL for {}: {message}",
                path.display()
            )
        })
    }
}

fn system_root() -> Option<PathBuf> {
    std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(PathBuf::from)
}
