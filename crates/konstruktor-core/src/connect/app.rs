//! The device-code flow for an *app*, as `/fakts-next/` runs it.
//!
//! A plugin engine is an app, not a hub: it is not a collection of service instances an
//! organization adopts, it is one client that asks to be let in and then talks to the
//! services a hub already runs. So it stages its device code at the app endpoint rather
//! than at `hub_authorization_endpoint`, posts a manifest describing itself rather than a
//! hub manifest full of instances, and what it gets back is an OAuth2 client — a
//! `client_id` and a `refresh_token` — rather than a rendered hub configuration.
//!
//! The polling half is RFC 8628 and identical in shape to the hub's; it is written out
//! again here rather than shared, because the two differ in exactly the part that
//! matters — what the token endpoint returns and how it is parsed — and a generic poll
//! would have to hand back untyped JSON to both.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::authorize::{WaitProgress, DEVICE_CODE_GRANT_TYPE};
use super::wellknown::{base_url, discover, CoordinationServerError};

/// What can go wrong claiming an app.
///
/// Its own type rather than the hub's, because every one of these reaches a person as a
/// sentence: an engine that was declined must not be told "the hub was declined", and a
/// refusal from the app endpoint is not a refused hub manifest. The shapes are the same
/// OAuth failures; the words are not interchangeable.
#[derive(Debug, thiserror::Error)]
pub enum AppAuthorizationError {
    #[error(transparent)]
    Server(#[from] CoordinationServerError),
    #[error("The coordination server refused the app manifest ({0}).")]
    Refused(String),
    #[error("The coordination server took the app manifest but returned no device code.")]
    NoDeviceCode,
    #[error("The app was declined.")]
    Declined,
    #[error("The authorization expired before anyone accepted it.")]
    Expired,
    #[error("Timed out waiting for the app to be accepted.")]
    TimedOut,
    #[error("Cancelled.")]
    Cancelled,
    #[error("The token endpoint answered {status}: {reason}")]
    TokenEndpoint { status: u16, reason: String },
    #[error(
        "The coordination server accepted the app but returned no refresh token, so the \
         container would have nothing to renew its access with. It answered with: {fields}"
    )]
    NoRefreshToken { fields: String },
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
}

/// The body the app device endpoint takes: a manifest, wrapped.
///
/// The manifest is a *field*, not the body — `DeviceCodeStartRequest.manifest` on the
/// server side. Posting the manifest flat is accepted as a request with unknown extra
/// keys and then refused for the one it is missing, which is what "1 validation error …
/// manifest Field required" means.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeStartRequest {
    pub manifest: AppManifest,
}

/// What an app says about itself when it asks to be let in.
///
/// The same shape fakts-next posts: who it is, which version, and what it wants to be
/// able to do. The coordination server shows this to whoever accepts the request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppManifest {
    /// Reverse-DNS, as every other Arkitekt manifest is — `live.arkitekt.deployer`.
    pub identifier: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    /// What the app is asking to be allowed to do.
    pub scopes: Vec<String>,
    /// Which installation this is. A second engine on another machine is a different
    /// instance of the same app.
    pub instance_id: String,
    /// This machine, so the same engine re-authorizing is recognised as itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

/// What the app authorization endpoint hands back — RFC 8628, plus where to poll.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppGrant {
    pub device_code: String,
    pub user_code: String,
    /// The public client the device code was staged against.
    #[serde(default)]
    pub client_id: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub verification_uri: String,
    /// Already absolute and already carrying the user code; open it verbatim.
    pub verification_uri_complete: String,
    #[serde(default)]
    pub expires_in: u64,
    #[serde(default)]
    pub interval: u64,
    /// From the well-known, not the endpoint: the `iss` the server declares.
    #[serde(default)]
    pub issuer: Option<String>,
}

/// The tokens an accepted app is given.
///
/// `client_id` and `refresh_token` are the two that outlive the run: they are what the
/// engine container is handed, so it can get itself an access token whenever it needs one
/// without a human at a browser. The access token in here expires within the hour and is
/// never written anywhere.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEnvelope {
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub scope: Option<String>,
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl AppEnvelope {
    /// What the token endpoint actually sent, for the error raised when the one field
    /// that matters is not among it.
    pub fn declared_fields(&self) -> String {
        let mut fields: Vec<&str> = self.extra.keys().map(String::as_str).collect();
        for (name, present) in [
            ("access_token", !self.access_token.is_empty()),
            ("client_id", !self.client_id.is_empty()),
            ("client_secret", self.client_secret.is_some()),
            ("scope", self.scope.is_some()),
        ] {
            if present {
                fields.push(name);
            }
        }
        fields.sort_unstable();
        if fields.is_empty() {
            "nothing".to_string()
        } else {
            fields.join(", ")
        }
    }
}

/// Stage a device code for an app.
pub async fn start(
    server: &str,
    manifest: &AppManifest,
) -> Result<AppGrant, AppAuthorizationError> {
    let well_known = discover(server).await?;
    let endpoint = well_known.app_device_endpoint().ok_or_else(|| {
        CoordinationServerError::NoAppAuthorization {
            server: base_url(server),
            declared: well_known.declared_keys(),
        }
    })?;

    let response = reqwest::Client::new()
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&DeviceCodeStartRequest {
            manifest: manifest.clone(),
        })
        .send()
        .await?;

    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or(serde_json::Value::Null);

    // A refusal can arrive as a non-2xx or as a 200 carrying `status: "error"`, the same
    // way the hub endpoint answers.
    let refused = body.get("status").and_then(|s| s.as_str()) == Some("error");
    if !status.is_success() || refused {
        let detail = body
            .get("error_description")
            .or_else(|| body.get("error"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| status.as_u16().to_string());
        return Err(AppAuthorizationError::Refused(detail));
    }

    let mut grant: AppGrant =
        serde_json::from_value(body).map_err(|_| AppAuthorizationError::NoDeviceCode)?;
    if grant.device_code.is_empty() || grant.token_endpoint.is_empty() {
        return Err(AppAuthorizationError::NoDeviceCode);
    }
    grant.issuer = well_known.issuer;
    Ok(grant)
}

enum Poll {
    Pending,
    SlowDown { interval: u64 },
    Granted(Box<AppEnvelope>),
}

async fn poll_once(grant: &AppGrant) -> Result<Poll, AppAuthorizationError> {
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
        let envelope: AppEnvelope =
            serde_json::from_value(payload).map_err(|e| AppAuthorizationError::TokenEndpoint {
                status: status.as_u16(),
                reason: format!("the envelope did not parse: {e}"),
            })?;
        return Ok(Poll::Granted(Box::new(envelope)));
    }

    match payload.get("error").and_then(|v| v.as_str()) {
        Some("authorization_pending") => Ok(Poll::Pending),
        Some("slow_down") => Ok(Poll::SlowDown {
            interval: grant.interval + 5,
        }),
        Some("access_denied") => Err(AppAuthorizationError::Declined),
        Some("expired_token") => Err(AppAuthorizationError::Expired),
        other => Err(AppAuthorizationError::TokenEndpoint {
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

/// Poll until somebody accepts the engine, the grant dies, or the caller gives up.
pub async fn wait_for_app(
    grant: &AppGrant,
    cancel: &tokio_util::sync::CancellationToken,
    on_waiting: &(dyn Fn(WaitProgress) + Sync),
) -> Result<AppEnvelope, AppAuthorizationError> {
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
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(total);
    let mut polls = 0;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Err(AppAuthorizationError::Cancelled),
            _ = tokio::time::sleep(std::time::Duration::from_secs(interval)) => {}
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(AppAuthorizationError::TimedOut);
        }

        match poll_once(grant).await? {
            Poll::Granted(envelope) => return Ok(*envelope),
            Poll::SlowDown { interval: next } => interval = next,
            Poll::Pending => {}
        }

        polls += 1;
        let now = tokio::time::Instant::now();
        on_waiting(WaitProgress {
            polls,
            seconds_left: (deadline - now).as_secs(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> AppManifest {
        AppManifest {
            identifier: "live.arkitekt.deployer".into(),
            version: "1.0.0".into(),
            description: None,
            logo: None,
            scopes: vec!["read".into()],
            instance_id: "my-engine".into(),
            node_id: Some("device".into()),
        }
    }

    /// The server takes a `DeviceCodeStartRequest`, whose `manifest` is a field. Sent
    /// flat, every key is an unknown extra and the one required field is missing — which
    /// is exactly the pydantic error a real server answered with.
    #[test]
    fn the_manifest_is_posted_as_a_field_not_as_the_body() {
        let body = serde_json::to_value(DeviceCodeStartRequest {
            manifest: manifest(),
        })
        .expect("serializes");

        assert_eq!(
            body["manifest"]["identifier"].as_str(),
            Some("live.arkitekt.deployer")
        );
        assert!(
            body.get("identifier").is_none(),
            "the manifest's own keys must not sit at the top level"
        );
    }
}
