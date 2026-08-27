#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    // The proxy helper inherits the askpass environment from its parent SSH
    // process, so it has to claim the launch first or it would print a secret
    // into the tunnel instead of forwarding bytes.
    if heminus_app::run_proxy_connect_if_requested() {
        return;
    }
    if heminus_app::run_askpass_if_requested() {
        return;
    }
    heminus_app::run();
}
