use serde::Serialize;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, EventTarget, Manager, State, WebviewWindow};

use crate::commands::TerminalTabTransferEvent;

/// A tab drop that landed in another Heminus window, reported by that window.
struct Landing {
    target_label: String,
    client_x: f64,
    client_y: f64,
    at: Instant,
}

#[derive(Default)]
pub(crate) struct DragResults {
    /// Per source window: whether GTK saw the drag miss every Heminus window.
    outside: Mutex<HashMap<String, bool>>,
    // ponytail: one slot, since a person drags one tab at a time.
    landing: Mutex<Option<Landing>>,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TerminalTabDrop {
    Outside,
    Cancelled,
    Landed {
        target_label: String,
        client_x: f64,
        client_y: f64,
    },
}

/// Webviews are separate GTK drop targets, so a drop on another window
/// succeeds as far as GTK is concerned; only that window knows it happened.
/// Screen coordinates cannot tell us that on Wayland.
fn poisoned<T>(_: T) -> String {
    "terminal tab drag state lock poisoned".into()
}

fn take_drop(results: &DragResults, source_label: &str) -> Result<Option<TerminalTabDrop>, String> {
    if let Some(outside) = results
        .outside
        .lock()
        .map_err(poisoned)?
        .remove(source_label)
    {
        return Ok(Some(if outside {
            TerminalTabDrop::Outside
        } else {
            TerminalTabDrop::Cancelled
        }));
    }
    let mut landing = results.landing.lock().map_err(poisoned)?;
    Ok(landing
        .take()
        .filter(|landing| {
            landing.target_label != source_label && landing.at.elapsed() < Duration::from_secs(3)
        })
        .map(|landing| TerminalTabDrop::Landed {
            target_label: landing.target_label,
            client_x: landing.client_x,
            client_y: landing.client_y,
        }))
}

fn is_terminal_window(label: &str) -> bool {
    label == "main" || label.starts_with("detached-")
}

pub fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("terminal-tab-drag")
        .setup(|app, _| {
            app.manage(DragResults::default());
            Ok(())
        })
        .on_webview_ready(|webview| {
            #[cfg(target_os = "linux")]
            {
                use gtk::prelude::*;
                let app = webview.app_handle().clone();
                let label = webview.label().to_string();
                if let Err(error) = webview.with_webview(move |platform| {
                    let widget = platform.inner();
                    let begin_app = app.clone();
                    let begin_label = label.clone();
                    widget.connect_drag_begin(move |_, _| {
                        let results = begin_app.state::<DragResults>();
                        if let Ok(mut outside) = results.outside.lock() {
                            outside.remove(&begin_label);
                        }
                        if let Ok(mut landing) = results.landing.lock() {
                            *landing = None;
                        }
                    });
                    widget.connect_drag_failed(move |_, _, result| {
                        if let Ok(mut outside) = app.state::<DragResults>().outside.lock() {
                            // Wayland reports a drop over another app's window as
                            // an error rather than NoTarget; both mean "outside".
                            let missed = !matches!(
                                result,
                                gtk::DragResult::UserCancelled | gtk::DragResult::GrabBroken
                            );
                            outside.insert(label.clone(), missed);
                        }
                        gtk::glib::Propagation::Proceed
                    });
                }) {
                    tracing::warn!("Could not observe terminal tab drag results: {error}");
                }
            }
            #[cfg(not(target_os = "linux"))]
            let _ = webview;
        })
        .on_event(|app, event| {
            if let tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Destroyed,
                ..
            } = event
                && let Ok(mut outside) = app.state::<DragResults>().outside.lock()
            {
                outside.remove(label);
            }
        })
        .build()
}

#[tauri::command]
pub fn record_terminal_tab_landing(
    window: WebviewWindow,
    state: State<'_, DragResults>,
    client_x: f64,
    client_y: f64,
) -> Result<(), String> {
    if !client_x.is_finite() || !client_y.is_finite() {
        return Err("Terminal tab drop position is invalid".into());
    }
    *state
        .landing
        .lock()
        .map_err(|_| "terminal tab drag state lock poisoned")? = Some(Landing {
        target_label: window.label().to_string(),
        client_x,
        client_y,
        at: Instant::now(),
    });
    Ok(())
}

#[tauri::command]
pub fn take_terminal_tab_drop(
    window: WebviewWindow,
    state: State<'_, DragResults>,
) -> Result<Option<TerminalTabDrop>, String> {
    take_drop(&state, window.label())
}

#[tauri::command]
pub fn transfer_terminal_tab_to(
    app: AppHandle,
    window: WebviewWindow,
    target_label: String,
    payload: String,
    client_x: f64,
    client_y: f64,
) -> Result<(), String> {
    if payload.len() > 16 * 1024 * 1024 {
        return Err("Terminal tab payload is too large".into());
    }
    if target_label == window.label() || !is_terminal_window(&target_label) {
        return Err("That window cannot take terminal tabs".into());
    }
    if app.get_webview_window(&target_label).is_none() {
        return Err("The target window has closed".into());
    }
    app.emit_to(
        EventTarget::webview_window(target_label),
        "terminal-tab-transfer",
        TerminalTabTransferEvent {
            payload,
            client_x,
            client_y,
        },
    )
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_reported_by_another_window_is_handed_to_the_source_once() {
        let results = DragResults::default();
        *results.landing.lock().unwrap() = Some(Landing {
            target_label: "detached-1".into(),
            client_x: 10.0,
            client_y: 20.0,
            at: Instant::now(),
        });
        assert_eq!(
            take_drop(&results, "main").unwrap(),
            Some(TerminalTabDrop::Landed {
                target_label: "detached-1".into(),
                client_x: 10.0,
                client_y: 20.0
            })
        );
        assert_eq!(take_drop(&results, "main").unwrap(), None);
    }

    #[test]
    fn gtk_failures_win_and_own_or_stale_landings_are_ignored() {
        let results = DragResults::default();
        results.outside.lock().unwrap().insert("main".into(), true);
        assert_eq!(
            take_drop(&results, "main").unwrap(),
            Some(TerminalTabDrop::Outside)
        );
        *results.landing.lock().unwrap() = Some(Landing {
            target_label: "main".into(),
            client_x: 0.0,
            client_y: 0.0,
            at: Instant::now(),
        });
        assert_eq!(take_drop(&results, "main").unwrap(), None);
        *results.landing.lock().unwrap() = Some(Landing {
            target_label: "detached-1".into(),
            client_x: 0.0,
            client_y: 0.0,
            at: Instant::now() - Duration::from_secs(10),
        });
        assert_eq!(take_drop(&results, "main").unwrap(), None);
    }
}
