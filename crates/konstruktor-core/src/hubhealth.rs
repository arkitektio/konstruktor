//! The hub telling its coordination server that it is alive.
//!
//! The coordination server counts a hub as online while it hears from it: a hub silent for
//! three report intervals is shown as offline, and while it is online the name it reports
//! for its tailnet node is what the server puts into its mesh aliases. So this runs *inside*
//! the stack — a `reporter` container beside the gateway — rather than in the app, which
//! is closed on most of the machines a hub actually runs on.
//!
//! It authenticates as the hub itself: the OAuth2 client the device-code grant created,
//! kept alive with its refresh token. The token rotates on every use, so the current one
//! lives in a state file of the reporter's own, seeded from `hub_credentials.json` —
//! which Konstruktor writes and never refreshes. A seed with a different `client_id` than
//! the one the state came from means the hub was authorized again, and the state is
//! re-seeded from it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::connect::wellknown;
use crate::credentials::{read_credentials, HubCredentials};

/// Where the reporter keeps the refresh token it last received.
pub const STATE_FILENAME: &str = "reporter.json";
/// Where the tailscale sidecar's LocalAPI socket is shared with the reporter.
pub const TAILSCALE_SOCKET: &str = crate::config::mesh::MESH_SOCKET;
/// The gateway, as the reporter reaches it on the stack's own network.
pub const GATEWAY_BASE: &str = "http://gateway";

/// Used when the server does not say how long to wait.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(60);
const MIN_INTERVAL: Duration = Duration::from_secs(15);
const MAX_INTERVAL: Duration = Duration::from_secs(600);
/// After a transport error: soon, but not in a tight loop.
const RETRY_AFTER_ERROR: Duration = Duration::from_secs(30);
/// After the refresh token was refused: nothing will fix itself short of the hub being
/// authorized again, so there is no point asking often.
const RETRY_AFTER_INVALID_GRANT: Duration = Duration::from_secs(300);

/// The reporter's own record of the rotating credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReporterState {
    pub client_id: String,
    pub refresh_token: String,
    /// The seed's `client_id` this state came from. A seed that says otherwise is a newer
    /// authorization.
    pub seeded_from: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error("no hub_credentials.json in {0} — has this hub been authorized?")]
    NoCredentials(PathBuf),
    #[error("the hub's grant carries no refresh token, so it cannot report on its own")]
    NoRefreshToken,
    #[error("the coordination server declares no token endpoint")]
    NoTokenEndpoint,
    #[error(
        "the coordination server refused the hub's refresh token ({0}). Authorize the hub \
         again — its old login was replaced or revoked."
    )]
    InvalidGrant(String),
    #[error("the coordination server did not accept the token ({0})")]
    Unauthorized(String),
    #[error("the coordination server answered {status}: {detail}")]
    Server { status: u16, detail: String },
    #[error(transparent)]
    Discovery(#[from] wellknown::CoordinationServerError),
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
    #[error("could not keep the reporter's state: {0}")]
    State(#[from] std::io::Error),
}

// --- state ---------------------------------------------------------------------------

/// The state to use: the stored one, unless the seed is from a newer authorization.
pub fn reconcile(seed: &HubCredentials, stored: Option<ReporterState>) -> Result<ReporterState, ReportError> {
    let seed_client = seed.envelope.client_id.clone();
    if let Some(state) = stored.filter(|s| s.seeded_from == seed_client) {
        return Ok(state);
    }
    let refresh_token = seed
        .envelope
        .refresh_token
        .clone()
        .filter(|t| !t.is_empty())
        .ok_or(ReportError::NoRefreshToken)?;
    Ok(ReporterState {
        client_id: seed_client.clone(),
        refresh_token,
        seeded_from: seed_client,
    })
}

pub fn read_state(dir: &Path) -> Option<ReporterState> {
    let text = std::fs::read_to_string(dir.join(STATE_FILENAME)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Written to a temporary file and renamed over the old one: a reporter killed half-way
/// through a write must not lose the only refresh token that still works.
pub fn write_state(dir: &Path, state: &ReporterState) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let target = dir.join(STATE_FILENAME);
    let partial = dir.join(format!("{STATE_FILENAME}.partial"));
    std::fs::write(&partial, serde_json::to_string_pretty(state).expect("serializes"))?;
    std::fs::rename(partial, target)
}

// --- endpoints -----------------------------------------------------------------------

/// Where health reports go: a declared `hub_health_endpoint`, or the sibling of the token
/// endpoint — `…/lok/o/token/` → `…/lok/f/hubhealth/`.
pub fn health_endpoint(well_known: &wellknown::WellKnownFakts) -> Option<String> {
    if let Some(declared) = well_known
        .extra
        .get("hub_health_endpoint")
        .and_then(|v| v.as_str())
    {
        return Some(declared.to_string());
    }
    let token = well_known.token_endpoint.as_deref()?;
    let base = token.trim_end_matches('/').strip_suffix("o/token")?;
    Some(format!("{base}f/hubhealth/"))
}

// --- tokens --------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

pub struct AccessToken {
    pub token: String,
    /// When to stop using it: a minute before it actually expires.
    pub renew_at: tokio::time::Instant,
}

/// Trades the refresh token for an access token, and the state for the rotated one. The
/// state is saved before the access token is used, so the rotation is never lost.
pub async fn refresh(
    client: &reqwest::Client,
    token_endpoint: &str,
    state: &mut ReporterState,
    state_dir: &Path,
) -> Result<AccessToken, ReportError> {
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", state.refresh_token.as_str()),
        ("client_id", state.client_id.as_str()),
    ];
    let response = client
        .post(token_endpoint)
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .await?;
    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or(serde_json::Value::Null);

    if !status.is_success() {
        let detail = error_detail(&body, status.as_u16());
        return Err(match body.get("error").and_then(|v| v.as_str()) {
            Some("invalid_grant") | Some("invalid_client") => ReportError::InvalidGrant(detail),
            _ => ReportError::Server {
                status: status.as_u16(),
                detail,
            },
        });
    }

    let granted: TokenResponse = serde_json::from_value(body).map_err(|e| ReportError::Server {
        status: status.as_u16(),
        detail: format!("the token response did not parse: {e}"),
    })?;
    if let Some(rotated) = granted.refresh_token.filter(|t| !t.is_empty()) {
        state.refresh_token = rotated;
        write_state(state_dir, state)?;
    }

    let lifetime = Duration::from_secs(granted.expires_in.unwrap_or(300));
    let margin = Duration::from_secs(60).min(lifetime / 2);
    Ok(AccessToken {
        token: granted.access_token,
        renew_at: tokio::time::Instant::now() + lifetime - margin,
    })
}

fn error_detail(body: &serde_json::Value, status: u16) -> String {
    body.get("error_description")
        .or_else(|| body.get("error"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| status.to_string())
}

// --- the report ----------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeshReport {
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<String>,
}

/// What `/f/hubhealth/` takes. `instances` is left out until the grant says which
/// instance token belongs to which service — today each entry carries only the token.
#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub healthy: bool,
    pub version: String,
    /// `null` on a hub without a mesh.
    pub mesh: Option<MeshReport>,
}

/// Every enabled service answers its health check through the gateway.
pub async fn services_healthy(client: &reqwest::Client, gateway: &str, hosts: &[String]) -> bool {
    for host in hosts {
        let url = format!(
            "{}/{host}/{}?format=json",
            gateway.trim_end_matches('/'),
            crate::health::HEALTH_PATH
        );
        let ok = client
            .get(&url)
            .send()
            .await
            .is_ok_and(|r| r.status().is_success());
        if !ok {
            return false;
        }
    }
    true
}

/// The tailnet node as `tailscale status --json` describes it, reduced to what the
/// coordination server wants: whether it is up, and the names it answers to.
pub fn mesh_from_status(status: &serde_json::Value) -> MeshReport {
    let connected = status.get("BackendState").and_then(|v| v.as_str()) == Some("Running");
    let own = status.get("Self");
    let hostname = own
        .and_then(|s| s.get("DNSName"))
        .and_then(|v| v.as_str())
        .map(|name| name.trim_end_matches('.').to_string())
        .filter(|name| !name.is_empty());
    let ipv4 = own
        .and_then(|s| s.get("TailscaleIPs"))
        .and_then(|v| v.as_array())
        .and_then(|ips| {
            ips.iter()
                .filter_map(|ip| ip.as_str())
                .find(|ip| ip.contains('.'))
                .map(str::to_string)
        });
    MeshReport {
        connected,
        hostname,
        ipv4,
    }
}

/// The tailnet node as the sidecar sees it, asked from the host — for `status`, the gateway
/// check and anything else outside the stack. `None` without a mesh, or when the sidecar
/// is not running to ask.
///
/// Through `compose exec` rather than the socket: the mesh interface and its LocalAPI live
/// in the sidecar's namespace, and nothing on the host can reach either directly.
pub async fn from_sidecar(
    dir: &Path,
    config: &crate::config::hub::HubConfig,
) -> Option<MeshReport> {
    let mesh = config.mesh.as_ref().filter(|m| m.enabled)?;
    from_sidecar_service(dir, &mesh.host).await
}

/// The same, for the sidecar under compose service `service` in the project in `dir` —
/// a hub's or an engine's.
pub async fn from_sidecar_service(dir: &Path, service: &str) -> Option<MeshReport> {
    let output = crate::engine_probe::engine()
        .async_command()
        .args([
            "compose",
            "exec",
            "-T",
            service,
            "sh",
            "-c",
            // The socket is where the compose file put it, or containerboot's default.
            "tailscale --socket \"${TS_SOCKET:-/tmp/tailscaled.sock}\" status --json",
        ])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let status: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    Some(mesh_from_status(&status))
}

/// Asks the sidecar's LocalAPI over its unix socket. `None` when there is no sidecar —
/// a hub without a mesh — which the report says as `mesh: null`.
#[cfg(unix)]
pub async fn mesh_status(socket: &Path) -> Option<MeshReport> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    if !socket.exists() {
        return None;
    }
    let exchange = async {
        let mut stream = tokio::net::UnixStream::connect(socket).await.ok()?;
        // HTTP/1.0, so the answer ends when the connection closes and no chunked decoding
        // is needed. The LocalAPI insists on this host name.
        stream
            .write_all(
                b"GET /localapi/v0/status HTTP/1.0\r\nHost: local-tailscaled.sock\r\n\r\n",
            )
            .await
            .ok()?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.ok()?;
        let text = String::from_utf8_lossy(&raw);
        let (head, body) = text.split_once("\r\n\r\n")?;
        if !head.starts_with("HTTP/1.") || !head.split(' ').nth(1).is_some_and(|c| c == "200") {
            return None;
        }
        serde_json::from_str::<serde_json::Value>(body).ok()
    };
    match tokio::time::timeout(Duration::from_secs(5), exchange).await {
        Ok(Some(status)) => Some(mesh_from_status(&status)),
        // A sidecar that is there but not answering is a mesh that is not connected.
        _ => Some(MeshReport {
            connected: false,
            hostname: None,
            ipv4: None,
        }),
    }
}

#[cfg(not(unix))]
pub async fn mesh_status(_socket: &Path) -> Option<MeshReport> {
    None
}

/// Sends one report. Answers how long to wait before the next.
pub async fn send_report(
    client: &reqwest::Client,
    endpoint: &str,
    access_token: &str,
    report: &HealthReport,
) -> Result<Duration, ReportError> {
    let response = client
        .post(endpoint)
        .bearer_auth(access_token)
        .json(report)
        .send()
        .await?;
    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or(serde_json::Value::Null);
    if status.as_u16() == 401 {
        return Err(ReportError::Unauthorized(error_detail(&body, 401)));
    }
    if !status.is_success() {
        return Err(ReportError::Server {
            status: status.as_u16(),
            detail: error_detail(&body, status.as_u16()),
        });
    }
    Ok(interval_from(&body))
}

pub fn interval_from(body: &serde_json::Value) -> Duration {
    body.get("next_report_in")
        .and_then(|v| v.as_u64())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_INTERVAL)
        .clamp(MIN_INTERVAL, MAX_INTERVAL)
}

// --- the loop ------------------------------------------------------------------------

/// Where the reporter reads from and writes to, and whom it asks.
pub struct ReporterConfig {
    /// Holds `hub_credentials.json` and `hub_config.yaml`, mounted read-only.
    pub seed_dir: PathBuf,
    /// The reporter's own volume.
    pub state_dir: PathBuf,
    pub gateway: String,
    pub tailscale_socket: PathBuf,
    pub version: String,
}

/// Reports until the process is stopped. Only a hub that was never authorized is an
/// error; everything else is logged and retried, because a reporter that exits is a hub
/// that goes offline for as long as nobody notices the container stopped.
pub async fn run(config: &ReporterConfig, log: &(dyn Fn(&str) + Sync)) -> Result<(), ReportError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let mut access: Option<AccessToken> = None;
    let mut last_seed_client = String::new();

    loop {
        // Re-read every round: re-authorizing rewrites the seed under a running reporter.
        let seed = read_credentials(&config.seed_dir)
            .ok_or_else(|| ReportError::NoCredentials(config.seed_dir.clone()))?;
        if seed.envelope.client_id != last_seed_client {
            access = None;
            last_seed_client = seed.envelope.client_id.clone();
        }

        let wait = match report_once(config, &client, &seed, &mut access, log).await {
            Ok(next) => next,
            Err(ReportError::InvalidGrant(detail)) => {
                log(&format!(
                    "the refresh token was refused ({detail}) — authorize this hub again; \
                     retrying in {}s",
                    RETRY_AFTER_INVALID_GRANT.as_secs()
                ));
                access = None;
                RETRY_AFTER_INVALID_GRANT
            }
            Err(error @ ReportError::NoRefreshToken) => return Err(error),
            Err(error) => {
                log(&format!("report failed: {error}; retrying in {}s", RETRY_AFTER_ERROR.as_secs()));
                RETRY_AFTER_ERROR
            }
        };
        tokio::time::sleep(wait).await;
    }
}

async fn report_once(
    config: &ReporterConfig,
    client: &reqwest::Client,
    seed: &HubCredentials,
    access: &mut Option<AccessToken>,
    log: &(dyn Fn(&str) + Sync),
) -> Result<Duration, ReportError> {
    let well_known = wellknown::discover(&seed.server).await?;
    let token_endpoint = well_known
        .token_endpoint
        .clone()
        .ok_or(ReportError::NoTokenEndpoint)?;
    let endpoint = health_endpoint(&well_known).ok_or(ReportError::NoTokenEndpoint)?;

    let hosts: Vec<String> = crate::profile::read_profile(&config.seed_dir)
        .map(|p| {
            p.config
                .enabled_services()
                .into_iter()
                .map(|id| p.config.service(id).host.clone())
                .collect()
        })
        .unwrap_or_default();

    let report = HealthReport {
        healthy: !hosts.is_empty() && services_healthy(client, &config.gateway, &hosts).await,
        version: config.version.clone(),
        mesh: mesh_status(&config.tailscale_socket).await,
    };

    for attempt in 0..2 {
        let token = match access.as_ref().filter(|a| tokio::time::Instant::now() < a.renew_at) {
            Some(token) => token.token.clone(),
            None => {
                let mut state = reconcile(seed, read_state(&config.state_dir))?;
                write_state(&config.state_dir, &state)?;
                let fresh = refresh(client, &token_endpoint, &mut state, &config.state_dir).await?;
                let token = fresh.token.clone();
                *access = Some(fresh);
                token
            }
        };
        match send_report(client, &endpoint, &token, &report).await {
            Ok(next) => {
                log(&format!(
                    "reported {} (mesh: {})",
                    if report.healthy { "healthy" } else { "unhealthy" },
                    match &report.mesh {
                        None => "none".to_string(),
                        Some(m) if m.connected => m.hostname.clone().unwrap_or_else(|| "connected".into()),
                        Some(_) => "not connected".to_string(),
                    }
                ));
                return Ok(next);
            }
            // An access token the server stopped accepting: get a fresh one, once.
            Err(ReportError::Unauthorized(_)) if attempt == 0 => *access = None,
            Err(error) => return Err(error),
        }
    }
    unreachable!("the loop returns on its second attempt")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::authorize::HubEnvelope;

    fn seed(client_id: &str, refresh: Option<&str>) -> HubCredentials {
        let envelope: HubEnvelope = serde_json::from_value(serde_json::json!({
            "token_type": "Bearer",
            "access_token": "eyJ",
            "client_id": client_id,
            "refresh_token": refresh,
        }))
        .unwrap();
        HubCredentials {
            version: 1,
            server: "coord.example.org".into(),
            identifier: "lab-hub".into(),
            authorized_at: "now".into(),
            issuer: None,
            envelope,
            advertised_hosts: Vec::new(),
        }
    }

    #[test]
    fn a_first_run_seeds_from_the_grant() {
        let state = reconcile(&seed("c1", Some("rt-0")), None).unwrap();
        assert_eq!(state.client_id, "c1");
        assert_eq!(state.refresh_token, "rt-0");
        assert_eq!(state.seeded_from, "c1");
    }

    /// The seed is never refreshed, so its token goes stale the moment the reporter
    /// rotates it. The stored one wins while the seed is the same authorization.
    #[test]
    fn a_rotated_token_outlives_the_stale_seed() {
        let stored = ReporterState {
            client_id: "c1".into(),
            refresh_token: "rt-7".into(),
            seeded_from: "c1".into(),
        };
        let state = reconcile(&seed("c1", Some("rt-0")), Some(stored.clone())).unwrap();
        assert_eq!(state, stored);
    }

    /// Re-authorizing replaces the client and deletes the old refresh chain.
    #[test]
    fn a_new_authorization_reseeds() {
        let stored = ReporterState {
            client_id: "c1".into(),
            refresh_token: "rt-7".into(),
            seeded_from: "c1".into(),
        };
        let state = reconcile(&seed("c2", Some("rt-new")), Some(stored)).unwrap();
        assert_eq!(state.client_id, "c2");
        assert_eq!(state.refresh_token, "rt-new");
    }

    #[test]
    fn a_grant_without_a_refresh_token_cannot_report() {
        assert!(matches!(
            reconcile(&seed("c1", None), None),
            Err(ReportError::NoRefreshToken)
        ));
    }

    #[test]
    fn the_health_endpoint_sits_beside_the_token_endpoint() {
        let known: wellknown::WellKnownFakts = serde_json::from_value(serde_json::json!({
            "token_endpoint": "https://go.arkitekt.live/lok/o/token/"
        }))
        .unwrap();
        assert_eq!(
            health_endpoint(&known).as_deref(),
            Some("https://go.arkitekt.live/lok/f/hubhealth/")
        );

        let declared: wellknown::WellKnownFakts = serde_json::from_value(serde_json::json!({
            "token_endpoint": "https://go.arkitekt.live/lok/o/token/",
            "hub_health_endpoint": "https://elsewhere.example.org/health/"
        }))
        .unwrap();
        assert_eq!(
            health_endpoint(&declared).as_deref(),
            Some("https://elsewhere.example.org/health/")
        );
    }

    #[test]
    fn reads_the_node_out_of_the_tailscale_status() {
        let status = serde_json::json!({
            "BackendState": "Running",
            "Self": {
                "DNSName": "myhub.iac.mesh.arkitekt.live.",
                "TailscaleIPs": ["fd7a:115c:a1e0::5", "100.64.0.5"]
            }
        });
        assert_eq!(
            mesh_from_status(&status),
            MeshReport {
                connected: true,
                hostname: Some("myhub.iac.mesh.arkitekt.live".into()),
                ipv4: Some("100.64.0.5".into()),
            }
        );

        let logging_in = serde_json::json!({ "BackendState": "NeedsLogin", "Self": {} });
        assert!(!mesh_from_status(&logging_in).connected);
    }

    #[test]
    fn the_interval_is_what_the_server_says_within_reason() {
        let at = |v: serde_json::Value| interval_from(&v).as_secs();
        assert_eq!(at(serde_json::json!({"next_report_in": 60})), 60);
        assert_eq!(at(serde_json::json!({})), 60);
        assert_eq!(at(serde_json::json!({"next_report_in": 1})), 15);
        assert_eq!(at(serde_json::json!({"next_report_in": 99999})), 600);
    }

    /// The report body is the contract: `healthy` required, `mesh` null without a sidecar,
    /// and no `instances` until the grant can name them.
    #[test]
    fn the_report_has_the_shape_the_server_takes() {
        let body = serde_json::to_value(HealthReport {
            healthy: true,
            version: "1.2.3".into(),
            mesh: None,
        })
        .unwrap();
        assert_eq!(
            body,
            serde_json::json!({"healthy": true, "version": "1.2.3", "mesh": null})
        );
    }
}
