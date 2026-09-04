#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let tray_start = std::env::args().any(|arg| arg.eq_ignore_ascii_case("--tray"));
    remote_env_desktop_lib::run_app(tray_start);
}
