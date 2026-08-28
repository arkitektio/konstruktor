use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::manifest::HubStartRequest;
use super::wellknown::{discover, CoordinationServerError};

/// Authorizing a hub with a coordination server, per `deployments/next/mounts/lok`:
///
/// 1. POST the hub manifest to `hub_authorization_endpoint` from the well-known. It
///    registers a public OAuth2 client and stages a device code of kind "hub".
/// 2. A human opens `verification_uri_complete` and picks the organization the hub joins.
/// 3. Poll the OAuth2 token endpoint with the device-code grant. On success the response
///    carries the tokens *and* the rendered hub config in one envelope.

pub const DEVICE_CODE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// What the authorization endpoint hands back — RFC 8628 plus the polling target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubGrant {
    /// Merged in from the well-known rather than returned by the endpoint: the exact
    /// string inbound tokens carry as their `iss` claim. Authentikate matches it by
    /// strict equality, so a hub that guesses it — say by using the bare hostname —
    /// rejects every token the coordination server issues.
    #[serde(default)]
    pub issuer: Option<String>,
    pub device_code: String,
    pub user_code: String,
    pub client_id: String,
    pub token_endpoint: String,
    pub verification_uri: String,
    /// Already absolute and already carrying the user code; open it verbatim.
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HubAuthClaim {
    /// Where the hub's services fetch the keys that verify inbound tokens.
    pub jwks_url: Option<String>,
    pub ionscale_auth_key: Option<String>,
    pub ionscale_coord_url: Option<String>,
}

/// The token response with the hub envelope appended. `instances` and `clients` are keyed
/// by the opaque token the coordination server minted, not by service name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubEnvelope {
    pub token_type: String,
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub scope: Option<String>,
    pub client_id: String,
    #[serde(default)]
    pub auth: HubAuthClaim,
    #[serde(default)]
    pub instances: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub clients: BTreeMap<String, serde_json::Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum HubAuthorizationError {
    #[error(transparent)]
    Server(#[from] CoordinationServerError),
    #[error("The coordination server refused the hub manifest ({0}).")]
    Refused(String),
    #[error("The coordination server staged the hub but returned no device code.")]
    NoDeviceCode,
    #[error("The hub was declined.")]
    Declined,
    #[error("The authorization expired before anyone accepted it.")]
    Expired,
    #[error("Timed out waiting for the hub to be accepted.")]
    TimedOut,
    #[error("Cancelled.")]
    Cancelled,
    #[error("The token endpoint answered {status}: {reason}")]
    TokenEndpoint { status: u16, reason: String },
    #[error(
        "The coordination server returned a grant with no JWKS URL; the hub's \
             services would trust nothing."
    )]
    NoJwksUrl,
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
}

/// Step 1 — stage the device code.
pub async fn start(
    server: &str,
    request: &HubStartRequest,
) -> Result<HubGrant, HubAuthorizationError> {
    let well_known = discover(server).await?;
    let endpoint = well_known.hub_authorization_endpoint.clone().ok_or(
        CoordinationServerError::NoHubAuthorization {
            server: server.to_string(),
        },
    )?;

    let response = reqwest::Client::new()
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(request)
        .send()
        .await?;

    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or(serde_json::Value::Null);

    // Both conditions, deliberately: the endpoint answers 200 with `status: "error"` for
    // a manifest it will not take.
    let granted = body.get("status").and_then(|s| s.as_str()) == Some("granted");
    if !status.is_success() || !granted {
        let detail = body
            .get("error_description")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| status.as_u16().to_string());
        return Err(HubAuthorizationError::Refused(detail));
    }

    let mut grant: HubGrant =
        serde_json::from_value(body).map_err(|_| HubAuthorizationError::NoDeviceCode)?;
    if grant.device_code.is_empty() || grant.token_endpoint.is_empty() {
        return Err(HubAuthorizationError::NoDeviceCode);
    }
    grant.issuer = well_known.issuer;
    Ok(grant)
}

pub enum PollStatus {
    Pending,
    SlowDown { interval: u64 },
    Granted(Box<HubEnvelope>),
}

/// One poll of the token endpoint. Terminal failures return `Err`.
pub async fn poll_once(grant: &HubGrant) -> Result<PollStatus, HubAuthorizationError> {
    // Form-encoded, per RFC 6749 — the token endpoint rejects JSON.
    let form = [
        ("grant_type", DEVICE_CODE_GRANT_TYPE),
        ("device_code", grant.device_code.as_str()),
        ("client_id", grant.client_id.as_str()),
    ];

    let response = reqwest::Client::new()
        .post(&grant.token_endpoint)
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .await?;

    let status = response.status();
    let payload: serde_json::Value = response.json().await.unwrap_or(serde_json::Value::Null);

    if status.is_success() {
        let envelope: HubEnvelope =
            serde_json::from_value(payload).map_err(|e| HubAuthorizationError::TokenEndpoint {
                status: status.as_u16(),
                reason: format!("the envelope did not parse: {e}"),
            })?;
        return Ok(PollStatus::Granted(Box::new(envelope)));
    }

    match payload.get("error").and_then(|v| v.as_str()) {
        Some("authorization_pending") => Ok(PollStatus::Pending),
        // Recomputed from the original grant every time, so repeated slow-downs do not
        // compound into a very long wait.
        Some("slow_down") => Ok(PollStatus::SlowDown {
            interval: grant.interval + 5,
        }),
        Some("access_denied") => Err(HubAuthorizationError::Declined),
        Some("expired_token") => Err(HubAuthorizationError::Expired),
        other => Err(HubAuthorizationError::TokenEndpoint {
            status: status.as_u16(),
            reason: payload
                .get("error_description")
                .and_then(|v| v.as_str())
                .or(other)
                .unwrap_or("no reason given")
                .to_string(),
        }),
    }
}

/// What a caller learns while the poll is running, so a UI or a terminal can stay honest.
pub struct WaitProgress {
    pub polls: u32,
    pub seconds_left: u64,
}

/// Step 3 — poll until a human accepts, the grant dies, or the caller gives up.
pub async fn wait_for_hub(
    grant: &HubGrant,
    cancel: &tokio_util::sync::CancellationToken,
    on_waiting: &(dyn Fn(WaitProgress) + Sync),
) -> Result<HubEnvelope, HubAuthorizationError> {
    let mut interval = if grant.interval > 0 {
        grant.interval
    } else {
        5
    };
    let total = if grant.expires_in > 0 {
        grant.expires_in
    } else {
        600
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(total);
    let mut polls = 0u32;

    loop {
        if cancel.is_cancelled() {
            return Err(HubAuthorizationError::Cancelled);
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err(HubAuthorizationError::TimedOut);
        }

        match poll_once(grant).await? {
            PollStatus::Granted(envelope) => {
                // `poll_once` accepts any 2xx as the envelope. A grant with no JWKS URL
                // would produce services that trust nothing, so refuse it here rather
                // than writing a deployment that cannot verify a single token.
                if envelope.auth.jwks_url.is_none() {
                    return Err(HubAuthorizationError::NoJwksUrl);
                }
                return Ok(*envelope);
            }
            PollStatus::SlowDown { interval: next } => interval = next,
            PollStatus::Pending => {}
        }

        polls += 1;
        on_waiting(WaitProgress {
            polls,
            seconds_left: (deadline - now).as_secs(),
        });

        // Abort-aware, unlike the TypeScript's plain `setTimeout`: Ctrl-C during a poll
        // interval used to land up to `interval` seconds late.
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(interval)) => {}
            _ = cancel.cancelled() => return Err(HubAuthorizationError::Cancelled),
        }
    }
}
