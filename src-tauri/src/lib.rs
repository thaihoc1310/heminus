mod commands;
mod credential;
mod desktop_integration;
mod key_management;
mod platform;
mod sftp;
mod ssh_runtime;
mod terminal;
mod tunnel;

use std::{collections::HashMap, sync::Mutex};

use heminus_storage::Database;
use sftp::SftpManager;
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use terminal::TerminalManager;
use tunnel::TunnelManager;

pub struct AppState {
    pub database: Mutex<Database>,
    pub ssh: ssh_runtime::SshRuntime,
    pub detached_payloads: Mutex<HashMap<String, String>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_target(false)
        .without_time()
        .compact()
        .init();

    let arguments: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let open_local_terminal_only = arguments
        .iter()
        .any(|argument| argument == "--new-terminal");
    let initial_cwd = arguments
        .iter()
        .position(|argument| argument == "--cwd")
        .and_then(|index| arguments.get(index + 1))
        .map(std::path::PathBuf::from);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed
                        && let Err(error) = commands::build_local_terminal_window(app, None)
                    {
                        tracing::warn!("Could not open the shortcut terminal: {error}");
                    }
                })
                .build(),
        )
        .setup(move |app| {
            let database = Database::open_default()?;
            let ssh = ssh_runtime::SshRuntime::initialize()?;
            key_management::migrate_external_keys(&database, &ssh)?;
            ssh_runtime::migrate_legacy_hosts(&database)?;
            database.migrate_welcome_host()?;
            database.reconcile_active_sessions()?;
            database.seed_welcome_hosts()?;
            app.manage(AppState {
                database: Mutex::new(database),
                ssh,
                detached_payloads: Mutex::new(HashMap::new()),
            });
            app.manage(TerminalManager::default());
            app.manage(TunnelManager::default());
            app.manage(SftpManager::default());
            match desktop_integration::ensure_local_terminal_shortcut() {
                Ok(true) => {}
                Ok(false) => {
                    if let Err(error) = app.global_shortcut().register("Ctrl+Alt+H") {
                        tracing::warn!("Could not register Ctrl+Alt+H: {error}");
                    }
                }
                Err(error) => {
                    tracing::warn!("Could not configure Ctrl+Alt+H: {error}");
                }
            }
            if open_local_terminal_only {
                commands::build_local_terminal_window(app.handle(), initial_cwd.as_deref())?;
                if let Some(main) = app.get_webview_window("main") {
                    main.destroy()?;
                }
            } else if let Some(main) = app.get_webview_window("main") {
                main.show()?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::platform_capabilities,
            commands::create_detached_terminal_window,
            commands::transfer_terminal_tab,
            commands::terminal_tab_pointer_state,
            commands::take_detached_terminal_payload,
            commands::home_directory,
            commands::local_path_info,
            commands::join_local_path,
            commands::list_hosts,
            commands::save_host,
            commands::delete_host,
            commands::list_groups,
            commands::save_group,
            commands::delete_group,
            commands::list_workspaces,
            commands::save_workspace,
            commands::delete_workspace,
            commands::list_identities,
            commands::save_identity,
            commands::delete_identity,
            commands::set_identity_secret,
            commands::delete_identity_secret,
            key_management::generate_ssh_key,
            key_management::import_private_key,
            key_management::read_key_material,
            commands::list_snippets,
            commands::save_snippet,
            commands::delete_snippet,
            commands::list_command_history,
            commands::list_all_command_history,
            commands::record_command_history,
            commands::delete_command_history,
            commands::clear_command_history,
            commands::list_port_forwards,
            commands::save_port_forward,
            commands::delete_port_forward,
            commands::list_sessions,
            commands::list_local_entries,
            commands::create_local_directory,
            commands::rename_local_entry,
            commands::remove_local_entry,
            commands::set_local_permissions,
            commands::open_local_entry,
            commands::list_known_hosts,
            commands::delete_known_host_entries,
            sftp::sftp_open,
            sftp::sftp_list,
            sftp::sftp_close,
            sftp::sftp_create_dir,
            sftp::sftp_remove,
            sftp::sftp_rename,
            sftp::sftp_set_permissions,
            sftp::sftp_upload,
            sftp::sftp_download,
            sftp::sftp_transfer_remote,
            sftp::sftp_cancel_transfer,
            terminal::terminal_open,
            terminal::terminal_attach,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_close,
            terminal::terminal_clipboard_write,
            terminal::terminal_clipboard_read,
            terminal::terminal_disconnect_history,
            terminal::terminal_rename,
            tunnel::tunnel_start,
            tunnel::tunnel_stop,
            tunnel::tunnel_states,
            tunnel::tunnel_port_processes,
            tunnel::tunnel_stop_port_processes,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Heminus");
}

pub fn run_askpass_if_requested() -> bool {
    credential::run_askpass_if_requested()
}
