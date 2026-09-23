use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `/.well-known/fakts` on the coordination server.
///
/// Endpoints are read off the well-known rather than assembled from the host: the paths
/// have moved before, and the token endpoint a grant must be polled at is whatever the
/// server says it is.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WellKnownFakts {
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub issuer: Option<String>,
    pub base_url: Option<String>,
    pub token_endpoint: Option<String>,
    pub jwks_uri: Option<String>,
    /// Where a hub manifest is POSTed to stage a device code.
    pub hub_authorization_endpoint: Option<String>,
    pub hub_configure: Option<String>,
    /// Where an *app* stages a device code — RFC 8628's name for it. A plugin engine is
    /// an app, not a hub, so this is the endpoint it uses.
    pub device_authorization_endpoint: Option<String>,
    /// Anything else the server declares, kept so a newer server round-trips intact.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// The other names a server might declare its app device endpoint under.
const APP_DEVICE_ENDPOINT_KEYS: [&str; 3] = [
    "app_authorization_endpoint",
    "device_endpoint",
    "claim_endpoint",
];

impl WellKnownFakts {
    /// Every endpoint-ish key the server declared, for the error that says none of them
    /// was an app device endpoint — which is the fastest way to find out what it is
    /// really called.
    pub fn declared_keys(&self) -> String {
        let mut keys: Vec<&str> = self.extra.keys().map(String::as_str).collect();
        if self.token_endpoint.is_some() {
            keys.push("token_endpoint");
        }
        if self.hub_authorization_endpoint.is_some() {
            keys.push("hub_authorization_endpoint");
        }
        keys.sort_unstable();
        if keys.is_empty() {
            "nothing".to_string()
        } else {
            keys.join(", ")
        }
    }

    /// Where an app stages a device code.
    ///
    /// Spelled several ways in the wild — RFC 8628 says `device_authorization_endpoint`,
    /// fakts has used names of its own — so the typed field is tried first and the rest
    /// read out of whatever else the server declared. Nothing is assembled from the host:
    /// the paths have moved before, which is why this document exists at all.
    pub fn app_device_endpoint(&self) -> Option<String> {
        if let Some(endpoint) = self.device_authorization_endpoint.as_ref() {
            return Some(endpoint.clone());
        }
        APP_DEVICE_ENDPOINT_KEYS
            .into_iter()
            .find_map(|key| self.extra.get(key).and_then(|value| value.as_str()))
            .map(str::to_string)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CoordinationServerError {
    #[error("Could not reach {server}: {source}")]
    Unreachable {
        server: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("{url} answered {status}. Is that an Arkitekt coordination server?")]
    NotFakts { url: String, status: u16 },
    #[error(
        "{server} does not offer hub authorization. It is either an older coordination \
         server or not one at all."
    )]
    NoHubAuthorization { server: String },
    #[error(
        "{server} declares no device endpoint for apps, so a plugin engine cannot ask \
         to be let in. Its well-known document lists: {declared}"
    )]
    NoAppAuthorization { server: String, declared: String },
    #[error("{url} did not answer with a well-known document: {source}")]
    Malformed {
        url: String,
        #[source]
        source: reqwest::Error,
    },
}

/// A bare host means https; anything with a scheme is taken as given.
pub fn base_url(server: &str) -> String {
    let trimmed = server.trim();
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    with_scheme.trim_end_matches('/').to_string()
}

pub fn well_known_url(server: &str) -> String {
    format!("{}/.well-known/fakts", base_url(server))
}

pub async fn discover(server: &str) -> Result<WellKnownFakts, CoordinationServerError> {
    let url = well_known_url(server);

    let response = reqwest::Client::new()
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|source| CoordinationServerError::Unreachable {
            server: base_url(server),
            source,
        })?;

    if !response.status().is_success() {
        return Err(CoordinationServerError::NotFakts {
            url,
            status: response.status().as_u16(),
        });
    }

    let well_known: WellKnownFakts =
        response
            .json()
            .await
            .map_err(|source| CoordinationServerError::Malformed {
                url: url.clone(),
                source,
            })?;

    // Deliberately not refused here for a missing `hub_authorization_endpoint`:
    // discovery is also what the picker uses to say what answered, and what an engine
    // uses to find the *app* endpoint. `authorize::start` is where a hub finds out its
    // server cannot authorize hubs, which is the only place that answer matters.
    Ok(well_known)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_host_is_reached_over_https() {
        assert_eq!(base_url("go.arkitekt.live"), "https://go.arkitekt.live");
        assert_eq!(base_url("  go.arkitekt.live  "), "https://go.arkitekt.live");
    }

    /// Somebody running a coordination server on their own machine pastes a full URL,
    /// port and all; forcing https onto it would make the address unusable.
    #[test]
    fn a_scheme_is_taken_as_given() {
        assert_eq!(base_url("http://localhost:8000"), "http://localhost:8000");
        assert_eq!(
            base_url("https://go.arkitekt.live/"),
            "https://go.arkitekt.live"
        );
    }

    #[test]
    fn builds_the_well_known_path() {
        assert_eq!(
            well_known_url("go.arkitekt.live"),
            "https://go.arkitekt.live/.well-known/fakts"
        );
    }
}
