//! The `bug_report` command. Everything it does is `konstruktor_core::report` — the
//! assembly, the redaction and the issue URL live there so `konstruktor report` produces
//! the identical document from a terminal.

use tauri::command;

pub use konstruktor_core::report::BugReport;

#[command]
pub async fn bug_report(
    app: tauri::AppHandle,
    path: String,
    service: String,
) -> Result<BugReport, String> {
    konstruktor_core::report::bug_report(
        &std::path::PathBuf::from(path),
        service,
        &app.package_info().version.to_string(),
    )
    .await
}
