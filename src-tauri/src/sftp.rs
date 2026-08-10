use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use heminus_domain::{Host, IdentityKind};
use russh_sftp::client::SftpSession;
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::AppState;

struct ChildStream {
    reader: ChildStdout,
    writer: ChildStdin,
}

impl AsyncRead for ChildStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.reader).poll_read(context, buffer)
    }
}

impl AsyncWrite for ChildStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        Pin::new(&mut self.writer).poll_write(context, buffer)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.writer).poll_flush(context)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.writer).poll_shutdown(context)
    }
}

struct SftpConnection {
    session: SftpSession,
    child: AsyncMutex<Child>,
    supervisor: crate::platform::ProcessSupervisor,
    _artifacts: crate::ssh_runtime::ConnectionArtifacts,
}

#[derive(Default)]
pub struct SftpManager {
    sessions: Mutex<HashMap<Uuid, Arc<SftpConnection>>>,
    transfers: Mutex<HashMap<Uuid, Arc<AtomicBool>>>,
}

impl SftpManager {
    fn get(&self, id: Uuid) -> Result<Arc<SftpConnection>, String> {
        self.sessions
            .lock()
            .map_err(|_| "SFTP session lock poisoned".to_string())?
            .get(&id)
            .cloned()
            .ok_or_else(|| "SFTP session not found".to_string())
    }

    fn begin_transfer(&self, id: Uuid) -> Result<Arc<AtomicBool>, String> {
        let mut transfers = self
            .transfers
            .lock()
            .map_err(|_| "SFTP transfer lock poisoned".to_string())?;
        if transfers.contains_key(&id) {
            return Err("SFTP transfer already exists".into());
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        transfers.insert(id, cancelled.clone());
        Ok(cancelled)
    }

    fn finish_transfer(&self, id: Uuid) {
        if let Ok(mut transfers) = self.transfers.lock() {
            transfers.remove(&id);
        }
    }

    fn cancel_transfer(&self, id: Uuid) -> Result<bool, String> {
        let transfers = self
            .transfers
            .lock()
            .map_err(|_| "SFTP transfer lock poisoned".to_string())?;
        let Some(cancelled) = transfers.get(&id) else {
            return Ok(false);
        };
        cancelled.store(true, Ordering::Release);
        Ok(true)
    }
}

const TRANSFER_CANCELLED: &str = "Transfer cancelled";

fn ensure_transfer_active(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        Err(TRANSFER_CANCELLED.into())
    } else {
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SftpOpenResult {
    id: Uuid,
    home_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEntry {
    name: String,
    path: String,
    is_directory: bool,
    is_symlink: bool,
    size: Option<u64>,
    modified_unix_ms: Option<u128>,
    mode: Option<u32>,
    owner: Option<String>,
    group: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferEvent {
    transferred: u64,
    total: u64,
}

#[tauri::command]
pub async fn sftp_open(
    manager: State<'_, SftpManager>,
    app_state: State<'_, AppState>,
    host: Host,
) -> Result<SftpOpenResult, String> {
    host.validate().map_err(|error| error.to_string())?;
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
        let host_arguments = app_state.ssh.sftp_host_arguments(&database, &host)?;
        let credential_environment =
            crate::credential::connection_askpass_environment(&database, &host, identity.as_ref())?;
        (identity, host_arguments, credential_environment)
    };
    let identity = identity.ok_or_else(|| {
        "Select and save a Password or Key identity on this host before opening SFTP. A local terminal does not authenticate an SSH connection.".to_string()
    })?;
    let username = identity.username.as_deref().unwrap_or(&host.username);
    let connection_artifacts = app_state.ssh.connection_artifacts(&host_arguments);

    let mut command = Command::new(crate::platform::ssh_executable()?);
    crate::platform::configure_background_command(command.as_std_mut());
    let password_identity = (identity.kind == IdentityKind::Password).then_some(&identity);
    for argument in app_state.ssh.sftp_arguments() {
        command.arg(argument);
    }
    for argument in host_arguments {
        command.arg(argument);
    }
    command
        .kill_on_drop(true)
        .env_remove("SSH_AUTH_SOCK")
        .env_remove("SSH_ASKPASS")
        .env_remove("SSH_ASKPASS_REQUIRE")
        .env_remove(crate::credential::askpass_candidates_env())
        .arg("-s")
        .arg("-T")
        .arg("-o")
        .arg(
            if password_identity.is_some() || credential_environment.is_some() {
                "BatchMode=no"
            } else {
                "BatchMode=yes"
            },
        )
        .arg("-o")
        .arg("ServerAliveInterval=30")
        .arg("-o")
        .arg("ServerAliveCountMax=3")
        .arg("-p")
        .arg(host.port.to_string());
    if let Some(key_path) = (identity.kind == IdentityKind::KeyFile)
        .then_some(identity.key_path.as_deref())
        .flatten()
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
    command.kill_on_drop(true);
    let mut child = command
        .arg("--")
        .arg(format!(
            "{username}@{}",
            crate::ssh_runtime::effective_address(&host)
        ))
        .arg("sftp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start OpenSSH SFTP: {error}"))?;
    let supervisor = match crate::platform::ProcessSupervisor::attach(child.id()) {
        Ok(supervisor) => supervisor,
        Err(error) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error);
        }
    };
    let reader = child
        .stdout
        .take()
        .ok_or_else(|| "Could not open the SFTP output stream".to_string())?;
    let writer = child
        .stdin
        .take()
        .ok_or_else(|| "Could not open the SFTP input stream".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Could not capture OpenSSH diagnostics".to_string())?;
    let stderr_task = tokio::spawn(async move {
        let mut output = Vec::new();
        let _ = stderr.read_to_end(&mut output).await;
        String::from_utf8_lossy(&output).trim().to_string()
    });
    let session = match tokio::time::timeout(
        Duration::from_secs(20),
        SftpSession::new(ChildStream { reader, writer }),
    )
    .await
    {
        Ok(Ok(session)) => session,
        Ok(Err(error)) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let diagnostics = stderr_task.await.unwrap_or_default();
            return Err(sftp_failure(
                &host,
                username,
                &diagnostics,
                Some(&error.to_string()),
            ));
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let diagnostics = stderr_task.await.unwrap_or_default();
            return Err(sftp_failure(
                &host,
                username,
                &diagnostics,
                Some("timed out"),
            ));
        }
    };
    session.set_timeout(30);
    let home_path = session
        .canonicalize(".")
        .await
        .map_err(|error| error.to_string())?;
    let id = Uuid::new_v4();
    manager
        .sessions
        .lock()
        .map_err(|_| "SFTP session lock poisoned")?
        .insert(
            id,
            Arc::new(SftpConnection {
                session,
                child: AsyncMutex::new(child),
                supervisor,
                _artifacts: connection_artifacts,
            }),
        );
    Ok(SftpOpenResult { id, home_path })
}

fn sftp_failure(host: &Host, username: &str, diagnostics: &str, fallback: Option<&str>) -> String {
    let detail = if diagnostics.is_empty() {
        fallback.unwrap_or("OpenSSH ended before starting the SFTP subsystem")
    } else {
        diagnostics
    };
    format!(
        "Could not open SFTP as {username}@{}: {detail}",
        crate::ssh_runtime::effective_address(host)
    )
}

#[tauri::command]
pub async fn sftp_list(
    manager: State<'_, SftpManager>,
    id: Uuid,
    path: String,
) -> Result<Vec<RemoteEntry>, String> {
    let connection = manager.get(id)?;
    let mut entries = connection
        .session
        .read_dir(path)
        .await
        .map_err(|error| error.to_string())?
        .map(|entry| {
            let metadata = entry.metadata();
            RemoteEntry {
                name: entry.file_name(),
                path: entry.path(),
                is_directory: metadata.is_dir(),
                is_symlink: metadata.is_symlink(),
                size: metadata.is_regular().then_some(metadata.len()),
                modified_unix_ms: metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_millis()),
                mode: metadata.permissions,
                owner: metadata
                    .user
                    .clone()
                    .or_else(|| metadata.uid.map(|uid| uid.to_string())),
                group: metadata
                    .group
                    .clone()
                    .or_else(|| metadata.gid.map(|gid| gid.to_string())),
            }
        })
        .filter(|entry| entry.name != "." && entry.name != "..")
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .is_directory
            .cmp(&left.is_directory)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(entries)
}

#[tauri::command]
pub async fn sftp_close(manager: State<'_, SftpManager>, id: Uuid) -> Result<bool, String> {
    let connection = manager
        .sessions
        .lock()
        .map_err(|_| "SFTP session lock poisoned".to_string())?
        .remove(&id);
    let Some(connection) = connection else {
        return Ok(false);
    };
    let _ = connection.session.close().await;
    connection.supervisor.terminate();
    let mut child = connection.child.lock().await;
    let _ = child.kill().await;
    let _ = child.wait().await;
    Ok(true)
}

#[tauri::command]
pub async fn sftp_create_dir(
    manager: State<'_, SftpManager>,
    id: Uuid,
    path: String,
) -> Result<(), String> {
    manager
        .get(id)?
        .session
        .create_dir(path)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn sftp_remove(
    manager: State<'_, SftpManager>,
    id: Uuid,
    path: String,
    _is_directory: bool,
) -> Result<(), String> {
    let connection = manager.get(id)?;
    remove_remote_entry(&connection, path).await
}

async fn remove_remote_tree(connection: &SftpConnection, path: String) -> Result<(), String> {
    let mut pending = vec![(path, false)];
    while let Some((current, visited)) = pending.pop() {
        if visited {
            connection
                .session
                .remove_dir(current)
                .await
                .map_err(|error| error.to_string())?;
            continue;
        }
        pending.push((current.clone(), true));
        let entries = connection
            .session
            .read_dir(current)
            .await
            .map_err(|error| error.to_string())?;
        for entry in entries {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let metadata = entry.metadata();
            if metadata.is_dir() && !metadata.is_symlink() {
                pending.push((entry.path(), false));
            } else {
                connection
                    .session
                    .remove_file(entry.path())
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(())
}

async fn remove_remote_entry(connection: &SftpConnection, path: String) -> Result<(), String> {
    let metadata = connection
        .session
        .metadata(path.clone())
        .await
        .map_err(|error| error.to_string())?;
    if metadata.is_dir() && !metadata.is_symlink() {
        remove_remote_tree(connection, path).await
    } else {
        connection
            .session
            .remove_file(path)
            .await
            .map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub async fn sftp_rename(
    manager: State<'_, SftpManager>,
    id: Uuid,
    old_path: String,
    new_path: String,
) -> Result<(), String> {
    manager
        .get(id)?
        .session
        .rename(old_path, new_path)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn sftp_set_permissions(
    manager: State<'_, SftpManager>,
    id: Uuid,
    path: String,
    mode: u32,
) -> Result<(), String> {
    if mode > 0o7777 {
        return Err("Permissions must be an octal mode between 0000 and 7777".into());
    }
    let connection = manager.get(id)?;
    let mut metadata = connection
        .session
        .metadata(path.clone())
        .await
        .map_err(|error| error.to_string())?;
    let existing = metadata.permissions.unwrap_or_default();
    metadata.permissions = Some((existing & !0o7777) | mode);
    connection
        .session
        .set_metadata(path, metadata)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn sftp_upload(
    manager: State<'_, SftpManager>,
    id: Uuid,
    transfer_id: Uuid,
    local_path: PathBuf,
    remote_path: String,
    on_progress: Channel<TransferEvent>,
) -> Result<(), String> {
    let cancelled = manager.begin_transfer(transfer_id)?;
    let result = async {
        let local_path = canonical_home_path(&local_path)?;
        let metadata = tokio::fs::metadata(&local_path)
            .await
            .map_err(|error| error.to_string())?;
        let connection = manager.get(id)?;
        let total = if metadata.is_dir() {
            local_tree_size(&local_path, &cancelled).await?
        } else {
            metadata.len()
        };
        let mut transferred = 0;
        ensure_transfer_active(&cancelled)?;

        if metadata.is_dir() {
            let staging_path = remote_temporary_path(&remote_path)?;
            ensure_remote_directory(&connection, &staging_path).await?;
            let transfer_result = async {
                let mut pending = vec![(local_path, staging_path.clone())];
                while let Some((local_directory, remote_directory)) = pending.pop() {
                    ensure_transfer_active(&cancelled)?;
                    let mut entries = tokio::fs::read_dir(&local_directory)
                        .await
                        .map_err(|error| error.to_string())?;
                    while let Some(entry) = entries
                        .next_entry()
                        .await
                        .map_err(|error| error.to_string())?
                    {
                        ensure_transfer_active(&cancelled)?;
                        let file_type =
                            entry.file_type().await.map_err(|error| error.to_string())?;
                        if file_type.is_symlink() {
                            continue;
                        }
                        let metadata = entry.metadata().await.map_err(|error| error.to_string())?;
                        let name = entry.file_name().to_string_lossy().into_owned();
                        let remote_child = remote_join_path(&remote_directory, &name);
                        if metadata.is_dir() {
                            ensure_remote_directory(&connection, &remote_child).await?;
                            pending.push((entry.path(), remote_child));
                        } else if metadata.is_file() {
                            upload_single_file(
                                &connection,
                                entry.path(),
                                remote_child,
                                total,
                                &mut transferred,
                                &on_progress,
                                &cancelled,
                            )
                            .await?;
                        }
                    }
                }
                Ok::<(), String>(())
            }
            .await;
            if let Err(error) = transfer_result {
                let _ = remove_remote_tree(&connection, staging_path).await;
                return Err(error);
            }
            commit_remote_staging(&connection, staging_path, remote_path, &cancelled).await?;
        } else {
            upload_single_file(
                &connection,
                local_path,
                remote_path,
                total,
                &mut transferred,
                &on_progress,
                &cancelled,
            )
            .await?;
        }
        ensure_transfer_active(&cancelled)?;
        if total == 0 {
            let _ = on_progress.send(TransferEvent {
                transferred: 0,
                total: 0,
            });
        }
        Ok(())
    }
    .await;
    manager.finish_transfer(transfer_id);
    result
}

async fn upload_single_file(
    connection: &SftpConnection,
    local_path: PathBuf,
    remote_path: String,
    total: u64,
    transferred: &mut u64,
    on_progress: &Channel<TransferEvent>,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    ensure_transfer_active(cancelled)?;
    let temporary_path = remote_temporary_path(&remote_path)?;
    let mut source = tokio::fs::File::open(local_path)
        .await
        .map_err(|error| error.to_string())?;
    let mut destination = connection
        .session
        .create(temporary_path.clone())
        .await
        .map_err(|error| error.to_string())?;
    let result = copy_with_progress(
        &mut source,
        &mut destination,
        total,
        transferred,
        on_progress,
        cancelled,
    )
    .await;
    if let Err(error) = result {
        let _ = connection.session.remove_file(&temporary_path).await;
        return Err(error);
    }
    if let Err(error) = ensure_transfer_active(cancelled) {
        let _ = connection.session.remove_file(&temporary_path).await;
        return Err(error);
    }
    if let Err(error) = destination.shutdown().await {
        let _ = connection.session.remove_file(&temporary_path).await;
        return Err(error.to_string());
    }
    commit_remote_staging(connection, temporary_path, remote_path, cancelled).await
}

#[tauri::command]
pub async fn sftp_download(
    manager: State<'_, SftpManager>,
    id: Uuid,
    transfer_id: Uuid,
    remote_path: String,
    local_path: PathBuf,
    on_progress: Channel<TransferEvent>,
) -> Result<(), String> {
    let cancelled = manager.begin_transfer(transfer_id)?;
    let result = async {
        let local_path = safe_new_local_path(&local_path)?;
        let connection = manager.get(id)?;
        let metadata = connection
            .session
            .metadata(remote_path.clone())
            .await
            .map_err(|error| error.to_string())?;
        let total = if metadata.is_dir() {
            remote_tree_size(&connection, &remote_path, &cancelled).await?
        } else {
            metadata.len()
        };
        let mut transferred = 0;
        ensure_transfer_active(&cancelled)?;

        if metadata.is_dir() {
            let staging_path = local_temporary_path(&local_path, "part")?;
            ensure_local_directory(&staging_path).await?;
            let transfer_result = async {
                let mut pending = vec![(remote_path, staging_path.clone())];
                while let Some((remote_directory, local_directory)) = pending.pop() {
                    ensure_transfer_active(&cancelled)?;
                    let entries = connection
                        .session
                        .read_dir(remote_directory)
                        .await
                        .map_err(|error| error.to_string())?;
                    for entry in entries {
                        ensure_transfer_active(&cancelled)?;
                        let name = entry.file_name();
                        if name == "." || name == ".." || name.contains('/') || name.contains('\\')
                        {
                            continue;
                        }
                        let metadata = entry.metadata();
                        let local_child = local_directory.join(&name);
                        if metadata.is_symlink() {
                            continue;
                        } else if metadata.is_dir() {
                            ensure_local_directory(&local_child).await?;
                            pending.push((entry.path(), local_child));
                        } else if metadata.is_regular() {
                            download_single_file(
                                &connection,
                                entry.path(),
                                local_child,
                                total,
                                &mut transferred,
                                &on_progress,
                                &cancelled,
                            )
                            .await?;
                        }
                    }
                }
                Ok::<(), String>(())
            }
            .await;
            if let Err(error) = transfer_result {
                let _ = remove_local_entry(&staging_path).await;
                return Err(error);
            }
            commit_local_staging(staging_path, local_path, &cancelled).await?;
        } else {
            download_single_file(
                &connection,
                remote_path,
                local_path,
                total,
                &mut transferred,
                &on_progress,
                &cancelled,
            )
            .await?;
        }
        ensure_transfer_active(&cancelled)?;
        if total == 0 {
            let _ = on_progress.send(TransferEvent {
                transferred: 0,
                total: 0,
            });
        }
        Ok(())
    }
    .await;
    manager.finish_transfer(transfer_id);
    result
}

async fn download_single_file(
    connection: &SftpConnection,
    remote_path: String,
    local_path: PathBuf,
    total: u64,
    transferred: &mut u64,
    on_progress: &Channel<TransferEvent>,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    ensure_transfer_active(cancelled)?;
    let temporary_path = local_path.with_file_name(format!(
        ".{}.heminus-{}.part",
        local_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("download"),
        Uuid::new_v4()
    ));
    let mut source = connection
        .session
        .open(remote_path)
        .await
        .map_err(|error| error.to_string())?;
    let mut destination = tokio::fs::File::create(&temporary_path)
        .await
        .map_err(|error| error.to_string())?;
    let result = copy_with_progress(
        &mut source,
        &mut destination,
        total,
        transferred,
        on_progress,
        cancelled,
    )
    .await;
    if let Err(error) = result {
        let _ = tokio::fs::remove_file(&temporary_path).await;
        return Err(error);
    }
    if let Err(error) = ensure_transfer_active(cancelled) {
        let _ = tokio::fs::remove_file(&temporary_path).await;
        return Err(error);
    }
    if let Err(error) = destination.flush().await {
        let _ = tokio::fs::remove_file(&temporary_path).await;
        return Err(error.to_string());
    }
    drop(destination);
    commit_local_staging(temporary_path, local_path, cancelled).await
}

#[tauri::command]
pub async fn sftp_transfer_remote(
    manager: State<'_, SftpManager>,
    source_id: Uuid,
    target_id: Uuid,
    transfer_id: Uuid,
    source_path: String,
    target_path: String,
    on_progress: Channel<TransferEvent>,
) -> Result<(), String> {
    if source_id == target_id && source_path == target_path {
        return Err("Source and destination are the same remote path".into());
    }
    let cancelled = manager.begin_transfer(transfer_id)?;
    let result = async {
        let source = manager.get(source_id)?;
        let target = manager.get(target_id)?;
        let metadata = source
            .session
            .metadata(source_path.clone())
            .await
            .map_err(|error| error.to_string())?;
        if source_id == target_id
            && metadata.is_dir()
            && remote_path_is_same_or_child(&source_path, &target_path)
        {
            return Err("A remote folder cannot be copied inside itself".into());
        }
        let total = if metadata.is_dir() {
            remote_tree_size(&source, &source_path, &cancelled).await?
        } else {
            metadata.len()
        };
        let mut transferred = 0;
        let transfer = RemoteTransferContext {
            source: &source,
            target: &target,
            total,
            on_progress: &on_progress,
            cancelled: &cancelled,
        };
        ensure_transfer_active(&cancelled)?;

        if metadata.is_dir() {
            let staging_path = remote_temporary_path(&target_path)?;
            ensure_remote_directory(&target, &staging_path).await?;
            let transfer_result = async {
                let mut pending = vec![(source_path, staging_path.clone())];
                while let Some((source_directory, target_directory)) = pending.pop() {
                    ensure_transfer_active(&cancelled)?;
                    let entries = source
                        .session
                        .read_dir(source_directory)
                        .await
                        .map_err(|error| error.to_string())?;
                    for entry in entries {
                        ensure_transfer_active(&cancelled)?;
                        let name = entry.file_name();
                        if name == "." || name == ".." || name.contains('/') || name.contains('\\')
                        {
                            continue;
                        }
                        let metadata = entry.metadata();
                        let target_child = remote_join_path(&target_directory, &name);
                        if metadata.is_symlink() {
                            continue;
                        } else if metadata.is_dir() {
                            ensure_remote_directory(&target, &target_child).await?;
                            pending.push((entry.path(), target_child));
                        } else if metadata.is_regular() {
                            transfer_remote_single_file(
                                &transfer,
                                entry.path(),
                                target_child,
                                &mut transferred,
                            )
                            .await?;
                        }
                    }
                }
                Ok::<(), String>(())
            }
            .await;
            if let Err(error) = transfer_result {
                let _ = remove_remote_tree(&target, staging_path).await;
                return Err(error);
            }
            commit_remote_staging(&target, staging_path, target_path, &cancelled).await?;
        } else {
            transfer_remote_single_file(&transfer, source_path, target_path, &mut transferred)
                .await?;
        }
        ensure_transfer_active(&cancelled)?;
        if total == 0 {
            let _ = on_progress.send(TransferEvent {
                transferred: 0,
                total: 0,
            });
        }
        Ok(())
    }
    .await;
    manager.finish_transfer(transfer_id);
    result
}

struct RemoteTransferContext<'a> {
    source: &'a SftpConnection,
    target: &'a SftpConnection,
    total: u64,
    on_progress: &'a Channel<TransferEvent>,
    cancelled: &'a AtomicBool,
}

async fn transfer_remote_single_file(
    transfer: &RemoteTransferContext<'_>,
    source_path: String,
    target_path: String,
    transferred: &mut u64,
) -> Result<(), String> {
    ensure_transfer_active(transfer.cancelled)?;
    let temporary_path = remote_temporary_path(&target_path)?;
    let mut reader = transfer
        .source
        .session
        .open(source_path)
        .await
        .map_err(|error| error.to_string())?;
    let mut writer = transfer
        .target
        .session
        .create(temporary_path.clone())
        .await
        .map_err(|error| error.to_string())?;
    let result = copy_with_progress(
        &mut reader,
        &mut writer,
        transfer.total,
        transferred,
        transfer.on_progress,
        transfer.cancelled,
    )
    .await;
    if let Err(error) = result {
        let _ = transfer.target.session.remove_file(&temporary_path).await;
        return Err(error);
    }
    if let Err(error) = ensure_transfer_active(transfer.cancelled) {
        let _ = transfer.target.session.remove_file(&temporary_path).await;
        return Err(error);
    }
    if let Err(error) = writer.shutdown().await {
        let _ = transfer.target.session.remove_file(&temporary_path).await;
        return Err(error.to_string());
    }
    commit_remote_staging(
        transfer.target,
        temporary_path,
        target_path,
        transfer.cancelled,
    )
    .await
}

async fn copy_with_progress<R, W>(
    reader: &mut R,
    writer: &mut W,
    total: u64,
    transferred: &mut u64,
    on_progress: &Channel<TransferEvent>,
    cancelled: &AtomicBool,
) -> Result<(), String>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut last_report = *transferred;
    loop {
        ensure_transfer_active(cancelled)?;
        let read = reader
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        writer
            .write_all(&buffer[..read])
            .await
            .map_err(|error| error.to_string())?;
        ensure_transfer_active(cancelled)?;
        *transferred += read as u64;
        if transferred.saturating_sub(last_report) >= 1024 * 1024 || *transferred == total {
            on_progress
                .send(TransferEvent {
                    transferred: *transferred,
                    total,
                })
                .map_err(|error| error.to_string())?;
            last_report = *transferred;
        }
    }
    if *transferred != last_report {
        let _ = on_progress.send(TransferEvent {
            transferred: *transferred,
            total,
        });
    }
    Ok(())
}

async fn local_tree_size(root: &Path, cancelled: &AtomicBool) -> Result<u64, String> {
    let mut total = 0_u64;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        ensure_transfer_active(cancelled)?;
        let mut entries = tokio::fs::read_dir(directory)
            .await
            .map_err(|error| error.to_string())?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| error.to_string())?
        {
            ensure_transfer_active(cancelled)?;
            let file_type = entry.file_type().await.map_err(|error| error.to_string())?;
            if file_type.is_symlink() {
                continue;
            }
            let metadata = entry.metadata().await.map_err(|error| error.to_string())?;
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

async fn remote_tree_size(
    connection: &SftpConnection,
    root: &str,
    cancelled: &AtomicBool,
) -> Result<u64, String> {
    let mut total = 0_u64;
    let mut pending = vec![root.to_string()];
    while let Some(directory) = pending.pop() {
        ensure_transfer_active(cancelled)?;
        let entries = connection
            .session
            .read_dir(directory)
            .await
            .map_err(|error| error.to_string())?;
        for entry in entries {
            ensure_transfer_active(cancelled)?;
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let metadata = entry.metadata();
            if metadata.is_symlink() {
                continue;
            } else if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_regular() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

#[tauri::command]
pub fn sftp_cancel_transfer(
    manager: State<'_, SftpManager>,
    transfer_id: Uuid,
) -> Result<bool, String> {
    manager.cancel_transfer(transfer_id)
}

async fn ensure_remote_directory(connection: &SftpConnection, path: &str) -> Result<(), String> {
    match connection.session.metadata(path.to_string()).await {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(format!(
            "Remote destination exists and is not a directory: {path}"
        )),
        Err(_) => connection
            .session
            .create_dir(path.to_string())
            .await
            .map_err(|error| error.to_string()),
    }
}

async fn ensure_local_directory(path: &Path) -> Result<(), String> {
    match tokio::fs::metadata(path).await {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(format!(
            "Local destination exists and is not a directory: {}",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => tokio::fs::create_dir(path)
            .await
            .map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    }
}

fn canonical_home_path(path: &Path) -> Result<PathBuf, String> {
    crate::platform::canonical_existing_local_path(path)
}

fn safe_new_local_path(path: &Path) -> Result<PathBuf, String> {
    crate::platform::safe_local_destination(path)
}

fn remote_join_path(parent: &str, child: &str) -> String {
    if parent.ends_with('/') {
        format!("{parent}{child}")
    } else {
        format!("{parent}/{child}")
    }
}

fn remote_path_is_same_or_child(parent: &str, candidate: &str) -> bool {
    let normalized_parent = parent.trim_end_matches('/');
    let normalized_candidate = candidate.trim_end_matches('/');
    normalized_candidate == normalized_parent
        || normalized_candidate
            .strip_prefix(normalized_parent)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn remote_temporary_path(remote_path: &str) -> Result<String, String> {
    remote_hidden_sibling(remote_path, "part")
}

async fn commit_remote_staging(
    connection: &SftpConnection,
    staging_path: String,
    target_path: String,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    if let Err(error) = ensure_transfer_active(cancelled) {
        let _ = remove_remote_entry(connection, staging_path).await;
        return Err(error);
    }
    let existing_backup = if connection
        .session
        .metadata(target_path.clone())
        .await
        .is_ok()
    {
        let backup_path = remote_hidden_sibling(&target_path, "backup")?;
        if let Err(error) = connection
            .session
            .rename(target_path.clone(), backup_path.clone())
            .await
        {
            let _ = remove_remote_entry(connection, staging_path).await;
            return Err(error.to_string());
        }
        Some(backup_path)
    } else {
        None
    };
    if let Err(error) = ensure_transfer_active(cancelled) {
        if let Some(backup_path) = &existing_backup {
            let _ = connection
                .session
                .rename(backup_path.clone(), target_path)
                .await;
        }
        let _ = remove_remote_entry(connection, staging_path).await;
        return Err(error);
    }
    if let Err(error) = connection
        .session
        .rename(staging_path.clone(), target_path.clone())
        .await
    {
        if let Some(backup_path) = &existing_backup {
            let _ = connection
                .session
                .rename(backup_path.clone(), target_path)
                .await;
        }
        let _ = remove_remote_entry(connection, staging_path).await;
        return Err(error.to_string());
    }
    if let Some(backup_path) = existing_backup {
        let _ = remove_remote_entry(connection, backup_path).await;
    }
    Ok(())
}

fn local_temporary_path(local_path: &Path, suffix: &str) -> Result<PathBuf, String> {
    let file_name = local_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Choose a destination file name".to_string())?;
    Ok(local_path.with_file_name(format!(".{file_name}.heminus-{}.{suffix}", Uuid::new_v4())))
}

async fn remove_local_entry(path: &Path) -> Result<(), String> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(|error| error.to_string())?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        tokio::fs::remove_dir_all(path)
            .await
            .map_err(|error| error.to_string())
    } else {
        tokio::fs::remove_file(path)
            .await
            .map_err(|error| error.to_string())
    }
}

async fn commit_local_staging(
    staging_path: PathBuf,
    target_path: PathBuf,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    if let Err(error) = ensure_transfer_active(cancelled) {
        let _ = remove_local_entry(&staging_path).await;
        return Err(error);
    }
    let existing_backup = if tokio::fs::symlink_metadata(&target_path).await.is_ok() {
        let backup_path = local_temporary_path(&target_path, "backup")?;
        if let Err(error) = tokio::fs::rename(&target_path, &backup_path).await {
            let _ = remove_local_entry(&staging_path).await;
            return Err(error.to_string());
        }
        Some(backup_path)
    } else {
        None
    };
    if let Err(error) = ensure_transfer_active(cancelled) {
        if let Some(backup_path) = &existing_backup {
            let _ = tokio::fs::rename(backup_path, &target_path).await;
        }
        let _ = remove_local_entry(&staging_path).await;
        return Err(error);
    }
    if let Err(error) = tokio::fs::rename(&staging_path, &target_path).await {
        if let Some(backup_path) = &existing_backup {
            let _ = tokio::fs::rename(backup_path, &target_path).await;
        }
        let _ = remove_local_entry(&staging_path).await;
        return Err(error.to_string());
    }
    if let Some(backup_path) = existing_backup {
        let _ = remove_local_entry(&backup_path).await;
    }
    Ok(())
}

fn remote_hidden_sibling(remote_path: &str, suffix: &str) -> Result<String, String> {
    let (parent, file_name) = remote_path
        .rsplit_once('/')
        .ok_or_else(|| "Choose a remote destination path".to_string())?;
    if file_name.is_empty() || file_name == "." || file_name == ".." {
        return Err("Choose a remote destination file name".into());
    }
    Ok(format!(
        "{parent}/.{file_name}.heminus-{}.{suffix}",
        Uuid::new_v4()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_paths_cannot_escape_the_user_profile() {
        assert!(canonical_home_path(Path::new("/")).is_err());
    }

    #[test]
    fn remote_uploads_use_a_hidden_partial_file() {
        let path = remote_temporary_path("/srv/data/archive.tar").expect("temporary path");
        assert!(path.starts_with("/srv/data/.archive.tar.heminus-"));
        assert!(path.ends_with(".part"));
    }

    #[test]
    fn remote_upload_destination_requires_a_file_name() {
        assert!(remote_temporary_path("/srv/data/").is_err());
    }

    #[test]
    fn local_staging_paths_are_hidden_siblings() {
        let path =
            local_temporary_path(Path::new("/tmp/archive"), "part").expect("local staging path");
        assert_eq!(path.parent(), Some(Path::new("/tmp")));
        let name = path.file_name().and_then(|name| name.to_str()).unwrap();
        assert!(name.starts_with(".archive.heminus-"));
        assert!(name.ends_with(".part"));
    }

    #[test]
    fn active_transfers_can_be_cancelled_and_removed() {
        let manager = SftpManager::default();
        let id = Uuid::new_v4();
        let cancelled = manager.begin_transfer(id).expect("transfer flag");
        assert!(!cancelled.load(Ordering::Acquire));
        assert!(manager.cancel_transfer(id).expect("cancel transfer"));
        assert!(cancelled.load(Ordering::Acquire));
        manager.finish_transfer(id);
        assert!(!manager.cancel_transfer(id).expect("finished transfer"));
    }

    #[test]
    fn remote_folders_cannot_be_copied_inside_themselves() {
        assert!(remote_path_is_same_or_child("/srv/data", "/srv/data/copy"));
        assert!(remote_path_is_same_or_child("/srv/data/", "/srv/data"));
        assert!(!remote_path_is_same_or_child("/srv/data", "/srv/database"));
        assert!(!remote_path_is_same_or_child("/srv/data", "/srv"));
    }
}
