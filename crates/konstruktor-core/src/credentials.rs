use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::connect::authorize::HubEnvelope;
use crate::generate::IssuedIdentity;

/// What the coordination server handed back when the hub was authorized.
///
/// Kept next to the profile as JSON rather than folded into `hub_config.yaml`: the profile
/// is a schema the Python CLI also reads and rejects unknown keys in, while this is
/// Konstruktor's own record of the grant.
pub const CREDENTIALS_FILENAME: &str = "hub_credentials.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubCredentials {
    pub version: u8,
    /// The coordination server, as the user gave it.
    pub server: String,
    /// The hub's identifier within the organization that accepted it.
    pub identifier: String,
    #[serde(rename = "authorizedAt")]
    pub authorized_at: String,
    /// The `iss` string the coordination server declares, from its well-known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    pub envelope: HubEnvelope,
    /// The addresses this hub last claimed to be reachable at.
    ///
    /// Kept so re-authorizing can start from what the hub already advertises instead of
    /// from whatever this machine happens to have today — which is the whole point of
    /// that screen, since the tailnet address it exists to add is one discovery cannot
    /// find. Defaulted, so a hub authorized before this existed still loads.
    #[serde(default)]
    pub advertised_hosts: Vec<crate::connect::manifest::AdvertisedHost>,
}

impl HubCredentials {
    /// What the generated service configs need out of a grant.
    pub fn issued_identity(&self) -> IssuedIdentity {
        IssuedIdentity {
            issuer: self.issuer.clone(),
            jwks_url: self.envelope.auth.jwks_url.clone(),
        }
    }
}

pub fn credentials_path(dir: &Path) -> PathBuf {
    dir.join(CREDENTIALS_FILENAME)
}

pub fn write_credentials(dir: &Path, credentials: &HubCredentials) -> std::io::Result<()> {
    let mut json = serde_json::to_string_pretty(credentials).expect("serializes");
    json.push('\n');
    std::fs::write(credentials_path(dir), json)
}

/// A deployment created before it was authorized simply has none.
pub fn read_credentials(dir: &Path) -> Option<HubCredentials> {
    let text = std::fs::read_to_string(credentials_path(dir)).ok()?;
    serde_json::from_str(&text).ok()
}
