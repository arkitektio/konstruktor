use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::connect::authorize::HubEnvelope;
use crate::generate::IssuedIdentity;

/// What the coordination server handed back each time the hub was connected.
///
/// Kept next to the profile as JSON rather than folded into `hub_config.yaml`: this is
/// Konstruktor's own record of the grants, with tokens in it.
pub const CREDENTIALS_FILENAME: &str = "hub_credentials.json";

/// The version written now: a hub connected to one coordination server, in as many of its
/// organizations as accepted it, one [`Grant`] each.
pub const CREDENTIALS_VERSION: u8 = 2;

/// A hub's grants. One coordination server; one grant per organization that accepted it.
///
/// The top-level grant fields (`authorizedAt`, `envelope`) are the first grant's, written as
/// version 1 had them: a health reporter pinned to an older build reads them and keeps
/// reporting for that organization. Everything that knows grants reads [`Self::grants`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubCredentials {
    pub version: u8,
    /// The coordination server, as the user gave it.
    pub server: String,
    /// What the hub is called at the coordination server.
    pub identifier: String,
    #[serde(rename = "authorizedAt")]
    pub authorized_at: String,
    /// The `iss` string the coordination server declares, from its well-known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    pub envelope: HubEnvelope,
    /// The addresses this hub last claimed to be reachable at.
    ///
    /// Kept so connecting again can start from what the hub already advertises instead of
    /// from whatever this machine happens to have today, which may no longer include an
    /// address somebody picked on purpose. Defaulted, so a hub authorized before this
    /// existed still loads.
    #[serde(default)]
    pub advertised_hosts: Vec<crate::connect::manifest::AdvertisedHost>,
    /// Every grant, the first one mirrored above. Empty in a version 1 file, whose one grant
    /// is the top-level one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grants: Vec<Grant>,
}

/// One organization's acceptance of the hub.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grant {
    #[serde(rename = "authorizedAt")]
    pub authorized_at: String,
    pub envelope: HubEnvelope,
}

impl Grant {
    /// The organization that accepted the hub: the `org` of its users' tokens. `None` from a
    /// coordination server that does not say.
    pub fn organization(&self) -> Option<&str> {
        self.envelope.self_.as_ref()?.organization.as_deref()
    }
}

impl HubCredentials {
    /// The credentials of a hub just connected for the first time.
    pub fn first(
        server: String,
        identifier: String,
        issuer: Option<String>,
        advertised_hosts: Vec<crate::connect::manifest::AdvertisedHost>,
        grant: Grant,
    ) -> Self {
        let mut credentials = Self {
            version: CREDENTIALS_VERSION,
            server,
            identifier,
            authorized_at: grant.authorized_at.clone(),
            issuer,
            envelope: grant.envelope.clone(),
            advertised_hosts,
            grants: Vec::new(),
        };
        credentials.set_grants(vec![grant]);
        credentials
    }

    /// Every grant the hub holds, in the order the organizations accepted it.
    pub fn grants(&self) -> Vec<Grant> {
        if self.grants.is_empty() {
            vec![Grant {
                authorized_at: self.authorized_at.clone(),
                envelope: self.envelope.clone(),
            }]
        } else {
            self.grants.clone()
        }
    }

    /// The organizations the hub was accepted into.
    pub fn organizations(&self) -> Vec<String> {
        self.grants()
            .iter()
            .filter_map(|grant| grant.organization().map(str::to_string))
            .collect()
    }

    /// Takes `grant` in: in place of the one of the same organization — accepting a hub
    /// again replaces what that organization knew of it — or beside the others. A grant that
    /// names no organization replaces the one other such grant, as version 1 had one grant.
    pub fn accept(&mut self, grant: Grant) {
        let mut grants = self.grants();
        match grants
            .iter()
            .position(|held| held.organization() == grant.organization())
        {
            Some(index) => grants[index] = grant,
            None => grants.push(grant),
        }
        self.set_grants(grants);
    }

    /// Lets go of the grant of `organization`, returning it. `None` when the hub holds none.
    /// The last grant cannot be let go of here: the hub then has no credentials at all
    /// ([`remove_credentials`]).
    pub fn release(&mut self, organization: &str) -> Option<Grant> {
        let mut grants = self.grants();
        let index = grants
            .iter()
            .position(|grant| grant.organization() == Some(organization))?;
        if grants.len() == 1 {
            return None;
        }
        let released = grants.remove(index);
        self.set_grants(grants);
        Some(released)
    }

    fn set_grants(&mut self, grants: Vec<Grant>) {
        if let Some(first) = grants.first() {
            self.authorized_at = first.authorized_at.clone();
            self.envelope = first.envelope.clone();
        }
        self.version = CREDENTIALS_VERSION;
        self.grants = grants;
    }

    /// What the generated service configs need out of the grants: one issuer, whose keys
    /// every grant shares — they are all from the one coordination server — and the
    /// organizations the services take tokens of.
    pub fn issued_identity(&self) -> IssuedIdentity {
        IssuedIdentity {
            issuer: self.issuer.clone(),
            jwks_url: self
                .grants()
                .iter()
                .find_map(|grant| grant.envelope.jwks_url().map(str::to_string)),
            organizations: self.organizations(),
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

/// A hub that was never connected simply has none.
pub fn read_credentials(dir: &Path) -> Option<HubCredentials> {
    let text = std::fs::read_to_string(credentials_path(dir)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Removes the record of every grant: what a hub that let go of its last one is left without.
pub fn remove_credentials(dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(credentials_path(dir)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(organization: &str, token: &str) -> Grant {
        Grant {
            authorized_at: format!("2026-10-20T00:00:0{organization}Z"),
            envelope: serde_json::from_value(serde_json::json!({
                "token_type": "Bearer",
                "access_token": token,
                "client_id": format!("client-{organization}"),
                "self": {
                    "jwks_url": "https://coord.example.org/.well-known/jwks.json",
                    "organization": organization,
                },
            }))
            .expect("an envelope"),
        }
    }

    fn connected_to(organization: &str) -> HubCredentials {
        HubCredentials::first(
            "coord.example.org".into(),
            "lab".into(),
            Some("https://coord.example.org".into()),
            Vec::new(),
            grant(organization, "a"),
        )
    }

    #[test]
    fn a_second_organization_adds_a_grant_and_the_first_stays_on_top() {
        let mut credentials = connected_to("3");
        credentials.accept(grant("4", "b"));

        assert_eq!(credentials.organizations(), ["3", "4"]);
        assert_eq!(credentials.envelope.client_id, "client-3");
        assert_eq!(credentials.issued_identity().organizations, ["3", "4"]);
    }

    #[test]
    fn accepted_again_an_organization_replaces_its_grant() {
        let mut credentials = connected_to("3");
        credentials.accept(grant("4", "b"));
        credentials.accept(grant("3", "c"));

        assert_eq!(credentials.organizations(), ["3", "4"]);
        assert_eq!(credentials.grants()[0].envelope.access_token, "c");
    }

    #[test]
    fn letting_go_of_the_first_grant_puts_the_next_on_top() {
        let mut credentials = connected_to("3");
        credentials.accept(grant("4", "b"));

        let released = credentials.release("3").expect("held");
        assert_eq!(released.organization(), Some("3"));
        assert_eq!(credentials.organizations(), ["4"]);
        assert_eq!(credentials.envelope.client_id, "client-4");
        assert!(credentials.release("3").is_none());
        // The last one is not let go of here: the hub then has no credentials at all.
        assert!(credentials.release("4").is_none());
    }

    #[test]
    fn a_version_one_file_is_its_one_grant() {
        let v1 = serde_json::json!({
            "version": 1,
            "server": "coord.example.org",
            "identifier": "lab",
            "authorizedAt": "2026-10-01T00:00:00Z",
            "envelope": {
                "token_type": "Bearer",
                "access_token": "a",
                "client_id": "c",
                "self": {"organization": "3"},
            },
        });
        let mut credentials: HubCredentials = serde_json::from_value(v1).expect("reads");
        assert_eq!(credentials.organizations(), ["3"]);
        assert_eq!(credentials.grants().len(), 1);

        credentials.accept(grant("4", "b"));
        assert_eq!(credentials.organizations(), ["3", "4"]);
    }

    #[test]
    fn written_the_first_grant_is_where_an_older_reporter_looks() {
        let mut credentials = connected_to("3");
        credentials.accept(grant("4", "b"));
        let written = serde_json::to_value(&credentials).expect("writes");

        assert_eq!(written["version"], 2);
        assert_eq!(written["envelope"]["client_id"], "client-3");
        assert_eq!(written["grants"].as_array().map(Vec::len), Some(2));
    }
}
