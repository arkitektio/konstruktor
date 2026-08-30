//! The tray: one icon, a menu that lists every hub and engine with its run state, and a
//! loop that keeps it honest while the window is hidden.
//!
//! Rust-driven on purpose. The window can be hidden for hours while stacks keep running
//! (`main.rs` leaves them up on close), and a tray fed from the webview would freeze the
//! moment it went away.

use std::sync::Mutex;
use std::time::Duration;

use konstruktor_core::status::{self, DeploymentStatus, RunState};
use tauri::{
    menu::{Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Emitter, Manager,
};
use tokio::sync::Notify;

pub const TRAY_ID: &str = "main";
/// The frontend listens for this to open a deployment the user picked from the tray.
pub const OPEN_EVENT: &str = "tray:open-deployment";

const INTERVAL: Duration = Duration::from_secs(10);
/// When nothing answers — no engine, most likely — asking every ten seconds spawns
/// processes for no news. Back off; a poke still refreshes immediately.
const BACKOFF: Duration = Duration::from_secs(30);

/// Managed on the app handle next to `StartedStacks`.
#[derive(Default)]
pub struct TrayState {
    /// The text the menu was last rendered from; a tick that renders the same text does
    /// not touch the menu, so an open menu is not yanked shut every ten seconds.
    signature: Mutex<String>,
    refresh: Notify,
}

/// Asks the loop to refresh now — after anything that changes what the menu shows.
pub fn poke(app: &AppHandle) {
    if let Some(state) = app.try_state::<TrayState>() {
        state.refresh.notify_one();
    }
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let menu = render(app, &[], true)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tauri::include_image!("icons/tray/tray@2x.png"))
        .icon_as_template(true)
        .tooltip("Konstruktor")
        .show_menu_on_left_click(true)
        .menu(&menu)
        .on_menu_event(on_menu_event)
        .build(app)?;

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let statuses = status::all().await;
            let _ = apply(&handle, &statuses);

            let all_failed = !statuses.is_empty() && statuses.iter().all(|s| s.error.is_some());
            let wait = if all_failed { BACKOFF } else { INTERVAL };
            let state = handle.state::<TrayState>();
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = state.refresh.notified() => {}
            }
        }
    });
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let id = event.id().as_ref();
    match id {
        "quit" => app.exit(0),
        "hide" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.hide();
            }
        }
        "show" => show_window(app),
        "refresh" => poke(app),
        other => {
            if let Some(deployment) = other.strip_prefix("open:") {
                show_window(app);
                let _ = app.emit(OPEN_EVENT, deployment.to_string());
            }
        }
    }
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn tray(app: &AppHandle) -> Option<TrayIcon> {
    app.tray_by_id(TRAY_ID)
}

/// Pushes a fresh status list into the tray, unless nothing visible changed.
fn apply(app: &AppHandle, statuses: &[DeploymentStatus]) -> tauri::Result<()> {
    let signature = statuses
        .iter()
        .map(|s| format!("{}|{}|{}", s.record.id, item_text(s), s.record.name))
        .collect::<Vec<_>>()
        .join("\n");

    let state = app.state::<TrayState>();
    if let Ok(mut last) = state.signature.lock() {
        if *last == signature {
            return Ok(());
        }
        *last = signature;
    }

    let Some(tray) = tray(app) else {
        return Ok(());
    };
    let menu = render(app, statuses, false)?;
    tray.set_menu(Some(menu))?;

    let running = statuses
        .iter()
        .filter(|s| s.run.state == RunState::Running)
        .count();
    // macOS shows text beside a template icon; elsewhere the tooltip carries it.
    #[cfg(target_os = "macos")]
    {
        let title = if running > 0 {
            format!("{running}")
        } else {
            String::new()
        };
        tray.set_title(Some(title))?;
    }
    tray.set_tooltip(Some(tooltip(statuses, running)))?;
    Ok(())
}

fn tooltip(statuses: &[DeploymentStatus], running: usize) -> String {
    if statuses.is_empty() {
        return "Konstruktor — no deployments".to_string();
    }
    let partial = statuses
        .iter()
        .filter(|s| s.run.state == RunState::Partial)
        .count();
    let mut parts = vec![format!("{running} running")];
    if partial > 0 {
        parts.push(format!("{partial} partly running"));
    }
    let rest = statuses.len() - running - partial;
    if rest > 0 {
        parts.push(format!("{rest} stopped"));
    }
    format!("Konstruktor — {}", parts.join(", "))
}

fn dot(status: &DeploymentStatus) -> &'static str {
    if status.error.is_some() {
        return "🔴";
    }
    match status.run.state {
        RunState::Running => "🟢",
        RunState::Partial => "🟡",
        RunState::Stopped => "⚪",
        RunState::None => "○",
    }
}

fn item_text(status: &DeploymentStatus) -> String {
    let detail = match (&status.error, status.run.state) {
        (Some(_), _) => "Unavailable".to_string(),
        (None, RunState::Running | RunState::Partial) => {
            format!("{} {}/{}", status.run.state.label(), status.run.running, status.run.total)
        }
        (None, state) => state.label().to_string(),
    };
    format!("{} {}  —  {}", dot(status), status.record.name, detail)
}

fn render(app: &AppHandle, statuses: &[DeploymentStatus], loading: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let mut menu = MenuBuilder::new(app);

    let running = statuses
        .iter()
        .filter(|s| s.run.state == RunState::Running)
        .count();
    let header = if loading {
        "Konstruktor — checking…".to_string()
    } else if statuses.is_empty() {
        "Konstruktor — no deployments yet".to_string()
    } else {
        format!("Konstruktor — {running} of {} running", statuses.len())
    };
    menu = menu
        .item(&MenuItemBuilder::with_id("header", header).enabled(false).build(app)?)
        .item(&PredefinedMenuItem::separator(app)?);

    let sections: [(&str, Vec<&DeploymentStatus>); 2] = [
        ("Hubs", statuses.iter().filter(|s| !s.is_engine()).collect()),
        ("Engines", statuses.iter().filter(|s| s.is_engine()).collect()),
    ];
    let mut any = false;
    for (title, items) in sections {
        if items.is_empty() {
            continue;
        }
        any = true;
        menu = menu.item(&MenuItemBuilder::with_id(format!("section:{title}"), title).enabled(false).build(app)?);
        for status in items {
            menu = menu.item(
                &MenuItemBuilder::with_id(format!("open:{}", status.record.id), item_text(status))
                    .build(app)?,
            );
        }
    }
    if any {
        menu = menu.item(&PredefinedMenuItem::separator(app)?);
    }

    menu = menu
        .item(&MenuItemBuilder::with_id("refresh", "Refresh now").build(app)?)
        .item(&MenuItemBuilder::with_id("show", "Open Konstruktor").build(app)?)
        .item(&MenuItemBuilder::with_id("hide", "Hide").build(app)?)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&MenuItemBuilder::with_id("quit", "Quit").build(app)?);
    menu.build()
}
