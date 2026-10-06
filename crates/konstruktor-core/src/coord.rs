//! A *coordination server*: the third kind of deployment, and the only one that is not a
//! client of another.
//!
//! A hub is a stack of services and a plugin engine is one deployer container; both are
//! authorized *against* a coordination server, which is where users, organizations and
//! permissions live. `go.arkitekt.live` is one. This module is for running your own.
//!
//! That inverts every assumption the other two paths make. A coordination server issues
//! the tokens rather than presenting them, so it has no device-code flow to complete, no
//! identifier inside somebody else's organization, and no manifest to send anywhere — it
//! is the root of trust, not a claimant. It also runs Lok, which
//! [`crate::generate::service`] notes a hub deliberately never does.
//!
//! # Half written
//!
//! Lok itself is generated: [`crate::generate::lok`] writes its config, its compose
//! service and its routes on the gateway, and a hub that runs its own — `hub create
//! --server local` — is a working stack, started and used end to end by the
//! `hub_local` test. That hub needs nobody's approval, because Konstruktor writes the
//! hub's manifest straight into Lok's config.
//!
//! A coordination server *on its own* is a different promise: other hubs authorize
//! against it, which means somebody opens the page Lok sends them to and accepts. That
//! page is not Lok's. Lok advertises it at the root of its address (`/hubconfigure/…`,
//! `/configure/…`) and expects the account frontend to be served there — and nothing in
//! this repository says which image that is, or how it is told where Lok lives.
//!
//! So everything here except [`create_coord`] is real — the folder is recognised as a
//! deployment, the registry records it, the lifecycle commands drive it — and
//! [`create_coord`] still returns an honest error. A stack without the frontend would
//! look right and accept nobody, which is worse.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::registry;

/// The registry's `kind` for a coordination server.
pub const COORD_KIND: &str = "coord";

/// Lok's config inside the deployment folder.
///
/// This doubles as how a folder is recognised as a coordination server rather than a
/// plugin engine — both are compose projects without a hub profile, so something has to
/// tell them apart, and each one's own config file is the natural marker.
/// See [`crate::profile::holds_a_deployment`].
pub const COORD_CONFIG_FILE: &str = "configs/lok.yaml";

/// Whether this folder holds a coordination server.
pub fn holds_a_coord(dir: &Path) -> bool {
    dir.join(COORD_CONFIG_FILE).is_file()
}

/// Everything a front end collects for a coordination server. Flat and serde-friendly,
/// like [`crate::create::HubAnswers`] and [`crate::engine::EngineAnswers`].
///
/// Deliberately short of the hub's questions. There is no coordination server to name
/// itself to, no services to pick, and nothing to advertise: what a coordination server
/// needs is somewhere to live, a port, and a first administrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordAnswers {
    pub dir: String,
    pub name: String,
    /// The public address clients will use. This is what hubs put in `--server`, so it
    /// has to be reachable from wherever they run, not just from here.
    #[serde(default)]
    pub domain: Option<String>,
    pub http_port: u16,
    pub https_port: u16,
    #[serde(default)]
    pub ssl: bool,
    /// The first account, which everything else is administered through.
    pub admin: String,
    #[serde(default)]
    pub admin_password: Option<String>,
    #[serde(default)]
    pub admin_email: Option<String>,
    /// Run `docker compose up -d` once everything is written.
    #[serde(default)]
    pub start: bool,
}

pub struct CreatedCoord {
    pub path: PathBuf,
    pub record: registry::DeploymentRecord,
}

#[derive(Debug, thiserror::Error)]
pub enum CoordError {
    /// The generator does not exist yet. Carries the list of what is missing rather than
    /// a bare "unimplemented", so the message a user sees is the same one a maintainer
    /// would need to act on.
    #[error(
        "Konstruktor cannot generate a coordination server on its own yet.\n\n\
         It can generate a hub that runs one: `konstruktor hub create --server local` \
         writes a stack with its own coordination server, which nobody has to accept and \
         which needs no network.\n\n\
         What is missing for a coordination server that *other* hubs authorize against is \
         the account frontend — the page somebody opens to accept a hub. Lok expects it at \
         the root of its address, and which image serves it is not known here.\n\n\
         Until then, point hubs at a coordination server you already run with \
         `konstruktor hub create --server <address>`."
    )]
    NotImplemented,
    #[error("{0}")]
    Folder(String),
    #[error("Could not write the deployment: {0}")]
    Write(#[from] std::io::Error),
}

/// Generate, write and register a coordination server.
///
/// # Not implemented
///
/// Returns [`CoordError::NotImplemented`]. The Lok half exists — see
/// [`crate::generate::lok`], which a self-contained hub is built from. To finish this, add
/// the account frontend to that stack (its image, its route at the gateway's root, and
/// Lok's `kontrol_frontend_url`), build a profile with no services, hand the files to
/// [`crate::generate::write::write_generated_files`], and register the folder with
/// [`registry::register_kind`] under [`COORD_KIND`] — writing [`COORD_CONFIG_FILE`], which
/// is what makes the folder recognisable afterwards.
///
/// Unlike the other two creators it takes no `CancellationToken` and no event callback for
/// an authorization, because there is nobody to authorize against; it should stream
/// `CreateEvent::Writing` and `CreateEvent::Starting` only.
pub async fn create_coord(_answers: &CoordAnswers) -> Result<CreatedCoord, CoordError> {
    Err(CoordError::NotImplemented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lok_config_marks_the_folder() {
        let dir = std::env::temp_dir().join(format!("konstruktor-coord-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("configs")).unwrap();
        assert!(!holds_a_coord(&dir));
        std::fs::write(dir.join(COORD_CONFIG_FILE), "lok: {}").unwrap();
        assert!(holds_a_coord(&dir));
        std::fs::remove_dir_all(&dir).ok();
    }
}
