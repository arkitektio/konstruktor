//! Filing a bug against the service that is misbehaving, not against this app.
//!
//! Every service is its own repository upstream — the profile already names it — so the
//! report goes where the code is. What makes it worth having is the log: a maintainer's
//! first question is always "what did it print", and the answer sits in a container the
//! reporter has to know a compose incantation to read.
//!
//! Which is also the danger. Django prints its settings on a crash, MinIO's init echoes
//! the keys it creates, and a caddy access log carries whatever token the browser sent.
//! Nothing leaves here without going through [`konstruktor_core::redact`], and the report
//! is shown to the user before anything is opened — a preview nobody reads is still the
//! difference between a leak and a decision.

use serde::{Deserialize, Serialize};

use crate::{docker, profile, redact};

/// How much of the log to take. Enough to hold a stack trace and what led to it; not so
/// much that nobody reads the preview before pressing the button.
const LOG_TAIL: u32 = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugReport {
    /// The compose service the report is about.
    pub service: String,
    /// Where its code lives, as the profile names it. `None` for a service the profile
    /// does not know — infrastructure, or an engine.
    pub repo: Option<String>,
    /// The "new issue" page, with the title and the environment already in it. The log is
    /// not: a query string cannot carry two hundred lines, so the body the user pastes
    /// comes off the clipboard.
    pub issue_url: Option<String>,
    pub title: String,
    /// The whole report as markdown — environment, then the redacted log. This is what
    /// goes on the clipboard, and what the preview shows.
    pub body: String,
    /// How many distinct secret values were taken out of the log. Shown, because "we
    /// removed your credentials" is a claim the user is entitled to see evidence for.
    pub redactions: usize,
    /// Why the log is missing, when it is. A hub whose Docker cannot be reached is
    /// exactly the thing somebody would be reporting, so this is not a failure.
    pub log_error: Option<String>,
}

/// Assemble a report for one service: its environment, and its log with the secrets out.
///
/// `client_version` is the front end's own version — the desktop app's bundle version, or
/// the CLI's crate version. It is the one thing the two callers cannot share.
pub async fn bug_report(
    dir: &std::path::Path,
    service: String,
    client_version: &str,
) -> Result<BugReport, String> {
    let path = dir.to_string_lossy().to_string();

    // The profile is what knows where a service's code lives. A folder without one is
    // still worth a report — it just cannot say which repository to file it against.
    let profile = profile::read_profile(dir).ok();
    // By `host`, which is the compose service name — that is what the dashboard passes
    // and what a container is labelled with, and it is not always the service's id.
    let block = profile.as_ref().and_then(|p| {
        crate::catalog::SERVICE_IDS
            .into_iter()
            .map(|id| p.config.service(id))
            .find(|block| block.host == service)
    });
    let repo = block.map(|b| b.github_repo.trim_end_matches('/').to_string());
    let image = block.and_then(|b| b.image.clone());

    let containers = docker::list_deployment_containers(&path)
        .await
        .unwrap_or_default();
    let container = containers
        .iter()
        .find(|c| c.service.as_deref() == Some(service.as_str()));

    let (log, log_error) = match read_log(dir, &service).await {
        Ok(text) => (text, None),
        Err(error) => (String::new(), Some(error)),
    };
    let redacted = redact::redact(&log, &redact::secrets_in_deployment(dir));

    let probe = docker::probe().await;
    let title = format!("{service}: ");
    let environment = environment(
        client_version,
        &service,
        image.as_deref(),
        container.and_then(|c| c.status.as_deref()),
        &probe,
    );

    let body = body(&environment, &redacted.text, log_error.as_deref(), &service);
    let issue_url = repo.as_ref().map(|repo| issue_url(repo, &title, &environment));

    Ok(BugReport {
        service,
        repo,
        issue_url,
        title,
        body,
        redactions: redacted.removed,
        log_error,
    })
}

/// `docker compose logs`, for one service.
async fn read_log(dir: &std::path::Path, service: &str) -> Result<String, String> {
    let mut args = crate::compose::logs(Some(service), LOG_TAIL);
    // Colour codes are invisible in a terminal and noise in a markdown code block.
    args.splice(1..1, ["--ansi".to_string(), "never".to_string()]);
    let output = crate::engine_probe::engine()
        .async_command()
        .args(&args)
        .current_dir(dir)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        // `--ansi never` is not always enough — some images write their own escapes.
        Ok(String::from_utf8_lossy(&strip_ansi_escapes::strip(&output.stdout)).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// The table maintainers ask for first, and the only part small enough to survive a URL.
fn environment(
    client_version: &str,
    service: &str,
    image: Option<&str>,
    status: Option<&str>,
    probe: &docker::DockerProbe,
) -> String {
    let engine = match (&probe.engine, probe.cli_version.as_deref()) {
        (Some(kind), Some(version)) => format!("{kind:?} {version}"),
        (Some(kind), None) => format!("{kind:?}"),
        (None, _) => "not found".to_string(),
    };

    let rows = [
        ("konstruktor", client_version.to_string()),
        ("platform", std::env::consts::OS.to_string()),
        ("engine", engine),
        (
            "compose",
            probe.compose_version.clone().unwrap_or_else(|| "unknown".into()),
        ),
        ("service", service.to_string()),
        ("image", image.unwrap_or("unknown").to_string()),
        ("container", status.unwrap_or("not running").to_string()),
    ];

    let mut table = String::from("| | |\n|---|---|\n");
    for (name, value) in rows {
        table.push_str(&format!("| {name} | `{value}` |\n"));
    }
    table
}

fn body(environment: &str, log: &str, log_error: Option<&str>, service: &str) -> String {
    let log_block = match log_error {
        Some(error) => format!("The log could not be read: `{error}`\n"),
        None if log.trim().is_empty() => "This service has written nothing.\n".to_string(),
        None => format!(
            "<details>\n<summary>docker compose logs {service} (last {LOG_TAIL} lines, \
             secrets removed)</summary>\n\n```\n{log}\n```\n\n</details>\n"
        ),
    };

    format!(
        "### What happened\n\n_Replace this with what you were doing and what you \
         expected._\n\n### Environment\n\n{environment}\n### Logs\n\n{log_block}\n\
         <sub>Filed from Konstruktor. The log was scanned against this deployment's own \
         configuration and every value it recognised was replaced with \
         `[redacted: …]`.</sub>\n"
    )
}

/// The new-issue page, carrying the title and the environment.
///
/// Deliberately not the log: GitHub answers a long enough query string with a 414, and a
/// report built to hold two hundred lines would hit it every time. The full body goes on
/// the clipboard instead, which is one paste and cannot fail.
fn issue_url(repo: &str, title: &str, environment: &str) -> String {
    let body = format!(
        "### What happened\n\n\n\n### Environment\n\n{environment}\n### Logs\n\n\
         _Paste here — Konstruktor put the redacted log on your clipboard._\n"
    );
    format!(
        "{repo}/issues/new?title={}&body={}",
        encode(title),
        encode(&body)
    )
}

/// Percent-encoding for a query value. Written out rather than pulled in: this encodes
/// everything that is not unreserved, which is always correct inside a query parameter.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_everything_a_query_cannot_carry() {
        assert_eq!(encode("a b&c=d#e"), "a%20b%26c%3Dd%23e");
        assert_eq!(encode("plain-value_1.0~"), "plain-value_1.0~");
    }

    /// The URL has to stay short: GitHub answers a long enough query string with a 414,
    /// and a report carrying two hundred lines of log would hit that every time.
    #[test]
    fn the_issue_url_carries_the_environment_and_not_the_log() {
        let log = "line of log\n".repeat(200);
        let url = issue_url(
            "https://github.com/arkitektio/rekuest-server-next",
            "rekuest: ",
            "| | |\n| image | `jhnnsrs/rekuest:next` |\n",
        );
        assert!(url.starts_with("https://github.com/arkitektio/rekuest-server-next/issues/new?"));
        assert!(url.contains("jhnnsrs%2Frekuest%3Anext"));
        assert!(!url.contains("line%20of%20log"));
        assert!(url.len() < 2000, "{} characters", url.len());
        let _ = log;
    }
}
