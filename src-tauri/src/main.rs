#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
mod cmd;
mod fix_env;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager,
};

fn main() {
    fix_env::fix().unwrap();

    tauri::Builder::default()
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let hide = MenuItemBuilder::with_id("hide", "Hide").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&hide)
                .item(&PredefinedMenuItem::separator(app)?)
                .item(&quit)
                .build()?;
            let _tray = TrayIconBuilder::new()
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "hide" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                    _ => {}
                })
                .build(app)?;
            Ok(())
        })
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
