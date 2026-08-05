use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use base64::Engine;
use heminus_domain::{
    Host, Identity, IdentityKind, PortForward, SessionRecord, Snippet, VaultGroup, Workspace,
};
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, EventTarget, Manager, State, WebviewWindow};
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    platform: &'static str,
    architecture: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalEntry {
    name: String,
    path: PathBuf,
    is_directory: bool,
    is_symlink: bool,
    size: Option<u64>,
    modified_unix_ms: Option<u128>,
    mode: Option<u32>,
    owner: Option<String>,
    group: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownHostEntry {
    hosts: String,
    key_type: String,
    fingerprint: String,
    line_number: usize,
    hashed: bool,
    resolved: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalTabTransferEvent {
    payload: String,
    client_x: f64,
    client_y: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalTabTransferResult {
    target_label: String,
    created_window: bool,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
    }
}

#[tauri::command]
pub async fn create_detached_terminal_window(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: String,
    title: String,
) -> Result<String, String> {
    build_detached_terminal_window(&app, state.inner(), payload, &title, None)
}

pub(crate) fn build_local_terminal_window(app: &AppHandle) -> Result<String, String> {
    let label = format!("detached-{}", Uuid::new_v4().simple());
    build_terminal_webview(app, &label, "Local Terminal", None, (720.0, 520.0))?;
    Ok(label)
}

fn build_detached_terminal_window(
    app: &AppHandle,
    state: &AppState,
    payload: String,
    title: &str,
    position: Option<(f64, f64)>,
) -> Result<String, String> {
    if payload.len() > 1_000_000 {
        return Err("Detached terminal payload is too large".into());
    }

    let label = format!("detached-{}", Uuid::new_v4().simple());
    state
        .detached_payloads
        .lock()
        .map_err(|_| "detached window state lock poisoned")?
        .insert(label.clone(), payload);

    let window_title = if title.trim().is_empty() {
        "Heminus Terminal"
    } else {
        title.trim()
    };
    if let Err(error) = build_terminal_webview(app, &label, window_title, position, (720.0, 520.0))
    {
        if let Ok(mut payloads) = state.detached_payloads.lock() {
            payloads.remove(&label);
        }
        return Err(error);
    }

    Ok(label)
}

fn build_terminal_webview(
    app: &AppHandle,
    label: &str,
    title: &str,
    position: Option<(f64, f64)>,
    size: (f64, f64),
) -> Result<(), String> {
    let mut builder = tauri::WebviewWindowBuilder::new(
        app,
        label,
        tauri::WebviewUrl::App(format!("index.html?detached={label}").into()),
    )
    .title(title)
    .inner_size(size.0, size.1)
    .min_inner_size(360.0, 240.0)
    .maximized(false)
    .background_color(tauri::webview::Color(0x00, 0x00, 0x00, 0x00))
    .transparent(true)
    .decorations(false)
    .resizable(true)
    .visible(false);
    if let Some((x, y)) = position {
        builder = builder.position(x, y);
    } else {
        builder = builder.center();
    }

    builder
        .build()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn transfer_terminal_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    window: WebviewWindow,
    payload: String,
    title: String,
    screen_x: f64,
    screen_y: f64,
) -> Result<TerminalTabTransferResult, String> {
    if payload.len() > 1_000_000 {
        return Err("Terminal tab payload is too large".into());
    }
    if !screen_x.is_finite() || !screen_y.is_finite() {
        return Err("Terminal tab drop position is invalid".into());
    }

    let source_scale = window.scale_factor().unwrap_or(1.0);
    let physical_x = screen_x * source_scale;
    let physical_y = screen_y * source_scale;
    for (label, target) in app.webview_windows() {
        if label == window.label()
            || (label != "main" && !label.starts_with("detached-"))
            || !target.is_visible().unwrap_or(true)
            || target.is_minimized().unwrap_or(false)
        {
            continue;
        }
        let Ok(position) = target.outer_position() else {
            continue;
        };
        let Ok(size) = target.outer_size() else {
            continue;
        };
        let left = f64::from(position.x);
        let top = f64::from(position.y);
        let right = left + f64::from(size.width);
        let bottom = top + f64::from(size.height);
        if physical_x < left || physical_x >= right || physical_y < top || physical_y >= bottom {
            continue;
        }

        let target_scale = target.scale_factor().unwrap_or(1.0);
        target
            .emit_to(
                EventTarget::webview_window(label.clone()),
                "terminal-tab-transfer",
                TerminalTabTransferEvent {
                    payload,
                    client_x: (physical_x - left) / target_scale,
                    client_y: (physical_y - top) / target_scale,
                },
            )
            .map_err(|error| error.to_string())?;
        return Ok(TerminalTabTransferResult {
            target_label: label,
            created_window: false,
        });
    }

    let label = build_detached_terminal_window(
        &app,
        state.inner(),
        payload,
        &title,
        Some((screen_x - 110.0, screen_y - 18.0)),
    )?;
    Ok(TerminalTabTransferResult {
        target_label: label,
        created_window: true,
    })
}

#[tauri::command]
pub fn take_detached_terminal_payload(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    if !window.label().starts_with("detached-") {
        return Ok(None);
    }
    state
        .detached_payloads
        .lock()
        .map_err(|_| "detached window state lock poisoned".to_string())
        .map(|mut payloads| payloads.remove(window.label()))
}

#[tauri::command]
pub fn home_directory() -> Result<PathBuf, String> {
    home_dir()
}

#[tauri::command]
pub fn list_hosts(state: State<'_, AppState>, query: Option<String>) -> Result<Vec<Host>, String> {
    let database = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?;
    database
        .list_hosts(query.as_deref())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_host(state: State<'_, AppState>, host: Host) -> Result<Host, String> {
    host.validate().map_err(|error| error.to_string())?;
    let database = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?;
    database
        .save_host(&host)
        .map_err(|error| error.to_string())?;
    Ok(host)
}

#[tauri::command]
pub fn delete_host(state: State<'_, AppState>, id: Uuid) -> Result<bool, String> {
    let database = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?;
    database.delete_host(id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_groups(state: State<'_, AppState>) -> Result<Vec<VaultGroup>, String> {
    state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?
        .list_groups()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_group(state: State<'_, AppState>, group: VaultGroup) -> Result<VaultGroup, String> {
    group.validate().map_err(|error| error.to_string())?;
    state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?
        .save_group(&group)
        .map_err(|error| error.to_string())?;
    Ok(group)
}

#[tauri::command]
pub fn delete_group(state: State<'_, AppState>, id: Uuid) -> Result<bool, String> {
    state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?
        .delete_group(id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_workspaces(state: State<'_, AppState>) -> Result<Vec<Workspace>, String> {
    state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?
        .list_workspaces()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_workspace(
    state: State<'_, AppState>,
    workspace: Workspace,
) -> Result<Workspace, String> {
    workspace.validate().map_err(|error| error.to_string())?;
    state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?
        .save_workspace(&workspace)
        .map_err(|error| error.to_string())?;
    Ok(workspace)
}

#[tauri::command]
pub fn delete_workspace(state: State<'_, AppState>, id: Uuid) -> Result<bool, String> {
    state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?
        .delete_workspace(id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_identities(state: State<'_, AppState>) -> Result<Vec<Identity>, String> {
    let database = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?;
    database
        .list_identities()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_identity(state: State<'_, AppState>, identity: Identity) -> Result<Identity, String> {
    identity.validate().map_err(|error| error.to_string())?;
    if identity.kind == IdentityKind::Agent {
        return Err(
            "System SSH-agent identities are not supported. Import the key into the Heminus vault."
                .into(),
        );
    }
    if let Some(path) = identity.key_path.as_deref() {
        let metadata = fs::metadata(path).map_err(|_| "The private key file does not exist")?;
        if !metadata.is_file() {
            return Err("The private key path must point to a file".into());
        }
        if !state.ssh.owns_key(Path::new(path)) {
            return Err("Private keys must be imported into the Heminus vault".into());
        }
    }
    let database = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?;
    database
        .save_identity(&identity)
        .map_err(|error| error.to_string())?;
    Ok(identity)
}

#[tauri::command]
pub async fn delete_identity(state: State<'_, AppState>, id: Uuid) -> Result<bool, String> {
    let identity = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?
        .find_identity(id)
        .map_err(|error| error.to_string())?;
    if identity
        .as_ref()
        .is_some_and(|identity| identity.secret_stored)
    {
        tauri::async_runtime::spawn_blocking(move || crate::credential::delete(id))
            .await
            .map_err(|error| error.to_string())??;
    }
    let database = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?;
    let deleted = database
        .delete_identity(id)
        .map_err(|error| error.to_string())?;
    drop(database);
    if deleted
        && let Some(path) = identity
            .and_then(|identity| identity.key_path)
            .map(PathBuf::from)
            .filter(|path| state.ssh.owns_key(path))
    {
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_file_name(format!(
            "{}.pub",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("key")
        )));
        let _ = fs::remove_file(path.with_file_name(format!(
            "{}-cert.pub",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("key")
        )));
    }
    Ok(deleted)
}

#[tauri::command]
pub async fn set_identity_secret(
    state: State<'_, AppState>,
    id: Uuid,
    secret: String,
) -> Result<bool, String> {
    let identity = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?
        .find_identity(id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Save the identity before storing its password".to_string())?;
    if !matches!(
        identity.kind,
        IdentityKind::Password | IdentityKind::KeyFile
    ) {
        return Err("Only password and key-file identities can store a credential".into());
    }
    if identity.kind == IdentityKind::KeyFile {
        let path = identity
            .key_path
            .ok_or_else(|| "The identity does not contain a private key".to_string())?;
        let validation_secret = secret.clone();
        tauri::async_runtime::spawn_blocking(move || {
            crate::key_management::validate_private_key_passphrase(
                std::path::Path::new(&path),
                &validation_secret,
            )
        })
        .await
        .map_err(|error| error.to_string())??;
    }
    tauri::async_runtime::spawn_blocking(move || crate::credential::set(id, secret))
        .await
        .map_err(|error| error.to_string())??;
    state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?
        .set_identity_secret_stored(id, true)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_identity_secret(state: State<'_, AppState>, id: Uuid) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || crate::credential::delete(id))
        .await
        .map_err(|error| error.to_string())??;
    state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?
        .set_identity_secret_stored(id, false)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_snippets(state: State<'_, AppState>) -> Result<Vec<Snippet>, String> {
    let database = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?;
    database.list_snippets().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_snippet(state: State<'_, AppState>, snippet: Snippet) -> Result<Snippet, String> {
    snippet.validate().map_err(|error| error.to_string())?;
    let database = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?;
    database
        .save_snippet(&snippet)
        .map_err(|error| error.to_string())?;
    Ok(snippet)
}

#[tauri::command]
pub fn delete_snippet(state: State<'_, AppState>, id: Uuid) -> Result<bool, String> {
    let database = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?;
    database
        .delete_snippet(id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_command_history(
    state: State<'_, AppState>,
    host_id: Option<Uuid>,
    limit: usize,
) -> Result<Vec<String>, String> {
    state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?
        .list_command_history(host_id, limit)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_all_command_history(
    state: State<'_, AppState>,
    limit: usize,
) -> Result<Vec<String>, String> {
    state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?
        .list_all_command_history(limit)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn record_command_history(
    state: State<'_, AppState>,
    host_id: Option<Uuid>,
    command: String,
) -> Result<bool, String> {
    state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?
        .record_command(host_id, &command)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_command_history(state: State<'_, AppState>, command: String) -> Result<bool, String> {
    state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?
        .delete_command_history(&command)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_port_forwards(state: State<'_, AppState>) -> Result<Vec<PortForward>, String> {
    let database = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?;
    database
        .list_port_forwards()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_port_forward(
    state: State<'_, AppState>,
    rule: PortForward,
) -> Result<PortForward, String> {
    rule.validate().map_err(|error| error.to_string())?;
    let database = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?;
    database
        .save_port_forward(&rule)
        .map_err(|error| error.to_string())?;
    Ok(rule)
}

#[tauri::command]
pub fn delete_port_forward(state: State<'_, AppState>, id: Uuid) -> Result<bool, String> {
    let database = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?;
    database
        .delete_port_forward(id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_sessions(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<SessionRecord>, String> {
    let database = state
        .database
        .lock()
        .map_err(|_| "database lock poisoned")?;
    database
        .list_sessions(limit.unwrap_or(200))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_local_entries(path: Option<PathBuf>) -> Result<Vec<LocalEntry>, String> {
    let requested = path.unwrap_or(home_dir()?);
    let resolved = fs::canonicalize(&requested).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    let user_names = unix_id_names("/etc/passwd");
    #[cfg(unix)]
    let group_names = unix_id_names("/etc/group");

    let mut entries = fs::read_dir(&resolved)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let metadata = entry.path().symlink_metadata().ok()?;
            let file_type = metadata.file_type();
            Some(LocalEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path(),
                is_directory: metadata.is_dir(),
                is_symlink: file_type.is_symlink(),
                size: metadata.is_file().then_some(metadata.len()),
                modified_unix_ms: metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_millis()),
                #[cfg(unix)]
                mode: Some(metadata.permissions().mode()),
                #[cfg(not(unix))]
                mode: None,
                #[cfg(unix)]
                owner: Some(
                    user_names
                        .get(&metadata.uid())
                        .cloned()
                        .unwrap_or_else(|| metadata.uid().to_string()),
                ),
                #[cfg(not(unix))]
                owner: None,
                #[cfg(unix)]
                group: Some(
                    group_names
                        .get(&metadata.gid())
                        .cloned()
                        .unwrap_or_else(|| metadata.gid().to_string()),
                ),
                #[cfg(not(unix))]
                group: None,
            })
        })
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
pub fn create_local_directory(path: PathBuf) -> Result<(), String> {
    let target = safe_local_destination(&path)?;
    fs::create_dir(&target).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn rename_local_entry(old_path: PathBuf, new_path: PathBuf) -> Result<(), String> {
    let source = safe_existing_local_entry(&old_path)?;
    let target = safe_local_destination(&new_path)?;
    fs::rename(source, target).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn remove_local_entry(path: PathBuf, _is_directory: bool) -> Result<(), String> {
    let home = fs::canonicalize(home_dir()?).map_err(|error| error.to_string())?;
    remove_local_entry_at(&path, &home)
}

fn remove_local_entry_at(path: &Path, protected_home: &Path) -> Result<(), String> {
    let target = safe_existing_local_entry(path)?;
    if target == protected_home || target.parent().is_none() {
        return Err("The home and root directories cannot be removed".into());
    }
    let metadata = fs::symlink_metadata(&target).map_err(|error| error.to_string())?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(target).map_err(|error| error.to_string())
    } else {
        fs::remove_file(target).map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub fn set_local_permissions(path: PathBuf, mode: u32) -> Result<(), String> {
    if mode > 0o7777 {
        return Err("Permissions must be an octal mode between 0000 and 7777".into());
    }
    let target = safe_existing_local_entry(&path)?;
    if fs::symlink_metadata(&target)
        .map_err(|error| error.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Changing permissions through a symbolic link is not supported".into());
    }
    #[cfg(unix)]
    {
        fs::set_permissions(target, fs::Permissions::from_mode(mode))
            .map_err(|error| error.to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = target;
        Err("Unix-style permissions are unavailable on this platform".into())
    }
}

#[tauri::command]
pub fn open_local_entry(path: PathBuf, application: Option<String>) -> Result<(), String> {
    let target = canonical_local_path(&path)?;
    let program = application
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("xdg-open");
    std::process::Command::new(program)
        .arg(target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open the item with {program}: {error}"))
}

#[tauri::command]
pub fn list_known_hosts(state: State<'_, AppState>) -> Result<Vec<KnownHostEntry>, String> {
    let Ok(contents) = fs::read_to_string(state.ssh.known_hosts_path()) else {
        return Ok(Vec::new());
    };
    let saved_hosts = {
        let database = state
            .database
            .lock()
            .map_err(|_| "database lock poisoned")?;
        database
            .list_hosts(None)
            .map_err(|error| error.to_string())?
    };
    Ok(contents
        .lines()
        .enumerate()
        .filter_map(|(line_number, line)| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.is_empty() || fields[0].starts_with('#') {
                return None;
            }
            let offset = usize::from(fields[0].starts_with('@'));
            let raw_hosts = fields.get(offset)?;
            let key_type = fields.get(offset + 1)?;
            let encoded_key = fields.get(offset + 2)?;
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(encoded_key)
                .ok()?;
            let digest = Sha256::digest(decoded);
            let fingerprint = base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest);
            let (display_hosts, hashed, resolved) = display_known_hosts(&saved_hosts, raw_hosts);
            Some(KnownHostEntry {
                hosts: display_hosts,
                key_type: (*key_type).to_string(),
                fingerprint: format!("SHA256:{fingerprint}"),
                line_number: line_number + 1,
                hashed,
                resolved,
            })
        })
        .collect())
}

#[tauri::command]
pub fn delete_known_host_entries(
    state: State<'_, AppState>,
    line_numbers: Vec<usize>,
) -> Result<usize, String> {
    let requested = line_numbers
        .into_iter()
        .filter(|line| *line > 0)
        .collect::<HashSet<_>>();
    if requested.is_empty() {
        return Ok(0);
    }
    let path = state.ssh.known_hosts_path();
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("Could not read the Heminus known-host vault: {error}"))?;
    let (updated, removed) = remove_known_host_lines(&contents, &requested);
    if removed == 0 {
        return Ok(0);
    }

    let temporary = path.with_file_name(format!(".known_hosts-{}.tmp", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let write_result = options
        .open(&temporary)
        .and_then(|mut file| file.write_all(updated.as_bytes()))
        .and_then(|()| fs::rename(&temporary, path));
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(format!(
            "Could not update the Heminus known-host vault: {error}"
        ));
    }
    Ok(removed)
}

fn remove_known_host_lines(contents: &str, requested: &HashSet<usize>) -> (String, usize) {
    let mut removed = 0;
    let mut retained = contents
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            if requested.contains(&(index + 1)) {
                removed += 1;
                None
            } else {
                Some(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !retained.is_empty() && contents.ends_with('\n') {
        retained.push('\n');
    }
    (retained, removed)
}

fn display_known_hosts(saved_hosts: &[Host], raw_hosts: &str) -> (String, bool, bool) {
    if !raw_hosts.starts_with("|1|") {
        return (raw_hosts.to_string(), false, true);
    }

    let mut displays = Vec::new();
    for host in saved_hosts {
        let effective_address = host
            .tags
            .iter()
            .find_map(|tag| tag.strip_prefix("hostname:"))
            .unwrap_or(&host.address);
        let display = known_host_name(effective_address, host.port);
        let probes = [&host.address, &host.label, effective_address];
        if probes
            .into_iter()
            .map(|probe| known_host_name(probe, host.port))
            .any(|probe| hashed_known_host_matches(raw_hosts, &probe))
            && !displays.contains(&display)
        {
            displays.push(display);
        }
    }

    if displays.is_empty() {
        ("Hashed host".to_string(), true, false)
    } else {
        (displays.join(", "), true, true)
    }
}

fn known_host_name(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

fn hashed_known_host_matches(encoded_host: &str, candidate: &str) -> bool {
    let fields = encoded_host.split('|').collect::<Vec<_>>();
    if fields.len() != 4 || !fields[0].is_empty() || fields[1] != "1" {
        return false;
    }
    let Ok(salt) = base64::engine::general_purpose::STANDARD.decode(fields[2]) else {
        return false;
    };
    let Ok(expected) = base64::engine::general_purpose::STANDARD.decode(fields[3]) else {
        return false;
    };
    let Ok(mut hmac) = Hmac::<Sha1>::new_from_slice(&salt) else {
        return false;
    };
    hmac.update(candidate.as_bytes());
    hmac.verify_slice(&expected).is_ok()
}

fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| Path::new(path).is_absolute())
        .ok_or_else(|| "HOME is unavailable".into())
}

#[cfg(unix)]
fn unix_id_names(path: &str) -> HashMap<u32, String> {
    fs::read_to_string(path)
        .ok()
        .into_iter()
        .flat_map(|contents| {
            contents
                .lines()
                .filter_map(|line| {
                    let fields = line.split(':').collect::<Vec<_>>();
                    Some((fields.get(2)?.parse().ok()?, fields.first()?.to_string()))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn canonical_local_path(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|error| error.to_string())
}

fn safe_existing_local_entry(path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| "Choose a valid file or folder".to_string())?;
    if file_name == "." || file_name == ".." {
        return Err("Choose a valid file or folder".into());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "Choose a valid parent directory".to_string())?;
    let target = canonical_local_path(parent)?.join(file_name);
    fs::symlink_metadata(&target).map_err(|error| error.to_string())?;
    Ok(target)
}

fn safe_local_destination(path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| "Choose a valid file or folder name".to_string())?;
    if file_name == "." || file_name == ".." {
        return Err("Choose a valid file or folder name".into());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "Choose a destination directory".to_string())?;
    Ok(canonical_local_path(parent)?.join(file_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_hashed_known_host_to_saved_effective_address() {
        let salt = b"heminus-known-host";
        let mut hmac = Hmac::<Sha1>::new_from_slice(salt).unwrap();
        hmac.update(b"[cluster-alias]:2222");
        let encoded = format!(
            "|1|{}|{}",
            base64::engine::general_purpose::STANDARD.encode(salt),
            base64::engine::general_purpose::STANDARD.encode(hmac.finalize().into_bytes())
        );
        let mut host = Host::new("Cluster", "cluster-alias", "root");
        host.port = 2222;
        host.tags.push("hostname:10.0.0.42".into());

        assert_eq!(
            display_known_hosts(&[host], &encoded),
            ("[10.0.0.42]:2222".into(), true, true)
        );
    }

    #[test]
    fn removes_only_selected_known_host_lines() {
        let requested = HashSet::from([2, 4]);
        let (updated, removed) = remove_known_host_lines(
            "host-a key-a\nhost-b key-b\n# comment\nhost-c key-c\n",
            &requested,
        );
        assert_eq!(removed, 2);
        assert_eq!(updated, "host-a key-a\n# comment\n");
    }

    #[test]
    fn hides_unresolved_known_host_hashes() {
        assert_eq!(
            display_known_hosts(&[], "|1|c2FsdA==|aGFzaA=="),
            ("Hashed host".into(), true, false)
        );
        assert_eq!(
            display_known_hosts(&[], "server.example.com,192.0.2.4"),
            ("server.example.com,192.0.2.4".into(), false, true)
        );
    }

    #[test]
    fn local_file_operations_accept_absolute_paths() {
        assert_eq!(
            canonical_local_path(Path::new("/")).unwrap(),
            PathBuf::from("/")
        );
        assert_eq!(
            safe_local_destination(Path::new("/tmp/heminus-file")).unwrap(),
            PathBuf::from("/tmp/heminus-file")
        );
    }

    #[cfg(unix)]
    #[test]
    fn deleting_a_directory_symlink_keeps_its_target() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("heminus-symlink-remove-{}", Uuid::new_v4()));
        let target = root.join("target");
        let link = root.join("link");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep.txt"), b"keep").unwrap();
        symlink(&target, &link).unwrap();

        remove_local_entry_at(&link, &root.join("protected-home")).unwrap();

        assert!(!link.exists());
        assert_eq!(fs::read(target.join("keep.txt")).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn renaming_a_symlink_moves_the_link_not_its_target() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("heminus-symlink-rename-{}", Uuid::new_v4()));
        let target = root.join("target");
        let source = root.join("source-link");
        let renamed = root.join("renamed-link");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("keep.txt"), b"keep").unwrap();
        symlink(&target, &source).unwrap();

        fs::rename(
            safe_existing_local_entry(&source).unwrap(),
            safe_local_destination(&renamed).unwrap(),
        )
        .unwrap();

        assert!(!source.exists());
        assert!(renamed.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(fs::read(target.join("keep.txt")).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
    }
}
