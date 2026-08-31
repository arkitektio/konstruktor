#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
mod cmd;
mod report;
mod fix_env;
mod tray;
use tauri::{Manager, RunEvent};

fn main() {
    // The login shell is read for `PATH` and the `DOCKER_*` variables. It can fail — a
    // profile that errors, a shell that prompts — and that is not a reason to have no
    // window: `engine_probe::candidates` is the floor under exactly this case.
    if let Err(error) = fix_env::fix() {
        eprintln!("could not read the login shell's environment: {error}");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(cmd::StartedStacks::default())
        .manage(tray::TrayState::default())
        .manage(cmd::InstallState::default())
        .manage(cmd::AuthorizeState::default())
        .setup(|app| {
            tray::init(app.handle())?;

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
            cmd::check_updates,
            cmd::probe_docker,
            cmd::probe_git,
            cmd::install_engine,
            cmd::cancel_install,
            cmd::cancel_authorization,
            cmd::start_engine,
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
            cmd::update_service,
            report::bug_report,
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
            cmd::compose_command_streamed,
            cmd::read_compose_file,
            cmd::read_compose_backup,
            cmd::write_compose_file,
            cmd::validate_compose_file,
            cmd::backup_folder,
            cmd::backup_deployment,
            cmd::read_backup_manifest,
            cmd::restore_plan,
            cmd::restore_deployment,
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
