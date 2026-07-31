fn main() {
    if heminus_app::run_askpass_if_requested() {
        return;
    }
    heminus_app::run();
}
