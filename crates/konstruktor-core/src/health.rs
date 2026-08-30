//! Whether a hub that is up is actually *working*.
//!
//! "Running" is what the container list says, and it is not the same question. A service
//! whose migrations fail against a restored database comes up, crashes, and is restarted
//! by its policy — `running` most of the time, and useless all of it. So this asks three
//! things in turn: do the containers stay up over a short hold, does Postgres accept
//! connections, and does each service answer an HTTP request through the gateway. The
//! URL is the one the dashboard's health dot polls, so the two never disagree.
//!
//! Written for the restore, which has to say "your services are still fine" or "they are
//! not, and here is which"; nothing in it is restore-specific.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::hub::{HubConfig, DB_COMPOSE_SERVICE};
use crate::connect::manifest::advertised_port;
use crate::docker::{self, Container};
use crate::engine_probe;
use crate::status::is_init_container;

/// How long to wait for every container to be running before giving up on that.
pub const CONTAINERS_TIMEOUT: Duration = Duration::from_secs(120);
/// How long the containers have to *stay* running before they count.
pub const HOLD: Duration = Duration::from_secs(10);
/// How long each service gets to answer through the gateway. Django's first request
/// after a restart runs migrations checks and warms caches; it is not quick.
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(90);

/// What was found out about one service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceHealth {
    /// The compose service — `rekuest`, or `db`.
    pub service: String,
    /// The last container state seen, e.g. `running`, `restarting`, `exited`.
    pub container_state: Option<String>,
    /// The container was seen leaving `running` during the hold — a crash loop, most
    /// likely, even if it happened to be `running` again at the last look.
    pub restarts_seen: bool,
    /// The HTTP status the service answered with through the gateway, when one was asked.
    pub http_status: Option<u16>,
    /// The URL that was asked, for the report.
    pub url: Option<String>,
    pub healthy: bool,
    /// One line for a person: why it is or is not healthy.
    pub detail: String,
}

/// One line of narration while the check runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum HealthEvent {
    Line { line: String },
    /// One service's verdict is in.
    Checked { service: String, healthy: bool, detail: String },
}

/// Runs the whole check. Never fails as such: a service that cannot be reached is a
/// result, not an error. The one thing that stops it is the engine not answering.
pub async fn check(
    dir: &Path,
    config: &HubConfig,
    on_event: &(dyn Fn(HealthEvent) + Send + Sync),
) -> Result<Vec<ServiceHealth>, String> {
    let path = dir.to_string_lossy().to_string();
    let say = |line: String| on_event(HealthEvent::Line { line });

    // --- 1. the containers, and that they stay up ------------------------------
    let mut seen_down: HashMap<String, bool> = HashMap::new();
    let mut last: HashMap<String, Container> = HashMap::new();
    let started = Instant::now();
    let mut all_up_since: Option<Instant> = None;

    say("Waiting for every container to be running…".into());
    loop {
        let containers = docker::list_deployment_containers(&path).await?;
        let counted: Vec<&Container> = containers
            .iter()
            .filter(|c| !is_init_container(c))
            .collect();

        let mut all_running = !counted.is_empty();
        for container in &counted {
            let name = container.service.clone().unwrap_or_default();
            let running = container.state.as_deref() == Some("running");
            // A container that was running and is not any more has crashed at least
            // once; that is remembered even if it is back by the next look.
            if let Some(previous) = last.get(&name) {
                if previous.state.as_deref() == Some("running") && !running {
                    seen_down.insert(name.clone(), true);
                }
            }
            if container.state.as_deref() == Some("restarting") {
                seen_down.insert(name.clone(), true);
            }
            all_running &= running;
            last.insert(name, (*container).clone());
        }

        if all_running {
            let since = *all_up_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= HOLD {
                break;
            }
        } else {
            all_up_since = None;
        }

        if started.elapsed() > CONTAINERS_TIMEOUT {
            say("Not every container came up in time.".into());
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    // --- 2. postgres ------------------------------------------------------------
    let db_ready = pg_isready(dir, &config.db.postgres_user).await;
    say(format!(
        "Postgres {}",
        if db_ready { "accepts connections" } else { "is not accepting connections" }
    ));

    let mut results = Vec::new();
    {
        let state = last.get(DB_COMPOSE_SERVICE);
        let restarts = seen_down.get(DB_COMPOSE_SERVICE).copied().unwrap_or(false);
        let healthy = db_ready && !restarts;
        let detail = if !db_ready {
            "pg_isready did not succeed".to_string()
        } else if restarts {
            "accepts connections, but the container was seen going down".to_string()
        } else {
            "accepts connections".to_string()
        };
        let verdict = ServiceHealth {
            service: DB_COMPOSE_SERVICE.into(),
            container_state: state.and_then(|c| c.state.clone()),
            restarts_seen: restarts,
            http_status: None,
            url: None,
            healthy,
            detail: detail.clone(),
        };
        on_event(HealthEvent::Checked {
            service: DB_COMPOSE_SERVICE.into(),
            healthy,
            detail,
        });
        results.push(verdict);
    }

    // --- 3. every service, through the gateway ------------------------------------
    let port = advertised_port(config);
    let scheme = if config.gateway.ssl { "https" } else { "http" };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| e.to_string())?;

    for id in config.enabled_services() {
        let host = config.service(id).host.clone();
        let url = format!("{scheme}://localhost:{port}/{host}/health/?format=json");
        let state = last.get(&host).and_then(|c| c.state.clone());
        let restarts = seen_down.get(&host).copied().unwrap_or(false);

        let status = wait_for_http(&client, &url).await;
        let answered = status.is_some_and(|s| (200..300).contains(&s));
        let healthy = answered && state.as_deref() == Some("running") && !restarts;
        let detail = match (status, restarts, state.as_deref()) {
            (None, _, Some("running")) => "did not answer through the gateway".to_string(),
            (None, _, Some(other)) => format!("container is {other}; nothing answered"),
            (None, _, None) => "no container".to_string(),
            (Some(s), true, _) => format!("answered {s}, but the container was seen going down"),
            (Some(s), false, _) if answered => format!("answered {s}"),
            (Some(s), false, _) => format!("answered {s} — not healthy"),
        };
        on_event(HealthEvent::Checked {
            service: host.clone(),
            healthy,
            detail: detail.clone(),
        });
        results.push(ServiceHealth {
            service: host,
            container_state: state,
            restarts_seen: restarts,
            http_status: status,
            url: Some(url),
            healthy,
            detail,
        });
    }

    Ok(results)
}

/// GETs until a non-5xx answer or the deadline. A 502/503 is the gateway saying the
/// service is not there yet, so it is retried rather than reported.
async fn wait_for_http(client: &reqwest::Client, url: &str) -> Option<u16> {
    let started = Instant::now();
    let mut last: Option<u16> = None;
    loop {
        if let Ok(response) = client.get(url).send().await {
            let status = response.status().as_u16();
            last = Some(status);
            if status < 500 {
                return last;
            }
        }
        if started.elapsed() > HTTP_TIMEOUT {
            return last;
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn pg_isready(dir: &Path, user: &str) -> bool {
    engine_probe::engine()
        .async_command()
        .args(["compose", "exec", "-T", DB_COMPOSE_SERVICE, "pg_isready", "-U", user])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}
