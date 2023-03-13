#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod cmd;

// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

fn main() {
    tauri::Builder::default()
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
