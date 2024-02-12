#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
use tauri::SystemTray;
mod cmd;
mod fix_env;
use tauri::{CustomMenuItem, SystemTrayMenu, SystemTrayMenuItem};

// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

fn main() {
    fix_env::fix().unwrap();

    let quit = CustomMenuItem::new("quit".to_string(), "Quit");
    let hide = CustomMenuItem::new("hide".to_string(), "Hide");
    let tray_menu = SystemTrayMenu::new()
        .add_item(quit)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(hide);

    let tray = SystemTray::new().with_menu(tray_menu);

    tauri::Builder::default()
        .system_tray(tray)
        .invoke_handler(tauri::generate_handler![
            cmd::hello_world_test,
            cmd::nana_test,
            cmd::restart_container,
            cmd::test_docker,
            cmd::docker_version_cmd,
            cmd::directory_init_cmd,
            cmd::directory_up_cmd,
            cmd::directory_stop_cmd,
            cmd::advertise_endpoint,
            cmd::list_network_interfaces,
            cmd::check_port_available
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
