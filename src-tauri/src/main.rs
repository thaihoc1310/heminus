#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    if heminus_app::run_askpass_if_requested() {
        return;
    }
    heminus_app::run();
}
