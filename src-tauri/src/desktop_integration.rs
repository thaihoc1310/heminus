#[cfg(target_os = "linux")]
pub fn ensure_local_terminal_shortcut() -> Result<bool, String> {
    use gio::prelude::*;

    const MEDIA_KEYS_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
    const CUSTOM_KEY_SCHEMA: &str =
        "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
    const SHORTCUT_PATH: &str =
        "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/heminus-terminal/";
    const SHORTCUT_BINDING: &str = "<Primary><Alt>h";
    const SHORTCUT_COMMAND: &str = "/usr/bin/heminus --new-terminal";
    const LEGACY_SHORTCUT_COMMAND: &str = "/usr/bin/heminus-app --new-terminal";

    if !std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains("gnome")
    {
        return Ok(false);
    }

    let media_keys = gio::Settings::new(MEDIA_KEYS_SCHEMA);
    let mut paths = media_keys
        .strv("custom-keybindings")
        .iter()
        .map(|path| path.to_string())
        .collect::<Vec<_>>();
    if paths.iter().any(|path| path == SHORTCUT_PATH) {
        let shortcut = gio::Settings::with_path(CUSTOM_KEY_SCHEMA, SHORTCUT_PATH);
        if shortcut.string("command").as_str() == LEGACY_SHORTCUT_COMMAND {
            shortcut
                .set_string("command", SHORTCUT_COMMAND)
                .map_err(|error| error.to_string())?;
            gio::Settings::sync();
        }
        return Ok(true);
    }

    for path in &paths {
        let shortcut = gio::Settings::with_path(CUSTOM_KEY_SCHEMA, path);
        if shortcut.string("binding").as_str() == SHORTCUT_BINDING {
            return Err(format!(
                "Ctrl+Alt+H is already assigned to {}",
                shortcut.string("name")
            ));
        }
    }

    let shortcut = gio::Settings::with_path(CUSTOM_KEY_SCHEMA, SHORTCUT_PATH);
    shortcut
        .set_string("name", "Open Heminus Local Terminal")
        .map_err(|error| error.to_string())?;
    shortcut
        .set_string("command", SHORTCUT_COMMAND)
        .map_err(|error| error.to_string())?;
    shortcut
        .set_string("binding", SHORTCUT_BINDING)
        .map_err(|error| error.to_string())?;
    paths.push(SHORTCUT_PATH.into());
    media_keys
        .set_strv(
            "custom-keybindings",
            paths.iter().map(String::as_str).collect::<Vec<_>>(),
        )
        .map_err(|error| error.to_string())?;
    gio::Settings::sync();
    Ok(true)
}

#[cfg(not(target_os = "linux"))]
pub const fn ensure_local_terminal_shortcut() -> Result<bool, String> {
    Ok(false)
}
