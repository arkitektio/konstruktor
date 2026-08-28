#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
mod cmd;
mod fix_env;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager, RunEvent,
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
        .manage(cmd::StartedStacks::default())
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

            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                window.open_devtools();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            cmd::list_deployment_containers,
            cmd::restart_container,
            cmd::deployment_images,
            cmd::probe_docker,
            cmd::probe_git,
            cmd::deployment_checkouts,
            cmd::checkout_branches,
            cmd::switch_checkout_branch,
            cmd::canonicalize_path,
            cmd::allow_deployment_dir,
            cmd::prepare_deployment_dir,
            cmd::discard_empty_dir,
            cmd::host_candidates,
            cmd::egress_identity,
            cmd::probe_reachability,
            cmd::create_hub,
            cmd::preview_hub_files,
            cmd::discover_server,
            cmd::mesh_domain,
            cmd::suggest_folder,
            cmd::identifier_from_folder,
            cmd::inspect_folder,
            cmd::list_deployments,
            cmd::forget_deployment,
            cmd::plan_deletion,
            cmd::delete_deployment,
            cmd::purge_deployment_data,
            cmd::hub_status,
            cmd::service_catalog,
            cmd::create_superuser,
            cmd::create_engine,
            cmd::reauthorize_hub,
            cmd::compose_command,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // `Exit`, not a window's `CloseRequested`: the tray has a Hide item, and on
            // macOS closing the window is not quitting — tearing the stack down there
            // would stop deployments the user is still using.
            if let RunEvent::Exit = event {
                let dirs = app.state::<cmd::StartedStacks>().take();
                if !dirs.is_empty() {
                    // Blocking on purpose. The process is on its way out, so anything
                    // handed to the async runtime would never get to run.
                    // No timeout override — each service's own `stop_grace_period`
                    // applies, so the database gets to close cleanly.
                    konstruktor_core::shutdown::stop_all(&dirs, None);
                }
            }
        });
}
