use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::hub::HubConfig;

/// The on-disk profile, in the envelope the `arkitekt-next` CLI reads
/// (`arkitekt_next/server/utils.py :: ProfileFile`):
///
/// ```yaml
/// version: '1.0'
/// kind: hub
/// backend: docker
/// config: { ...full model dump... }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub version: String,
    pub kind: String,
    pub backend: String,
    pub config: HubConfig,
}

pub const HUB_CONFIG_FILENAME: &str = "hub_config.yaml";

pub fn profile_path(dir: &Path) -> PathBuf {
    dir.join(HUB_CONFIG_FILENAME)
}

/// The envelope a freshly built config is stored in, so the Python CLI can still read the
/// folder.
pub fn hub_profile(config: HubConfig) -> Profile {
    Profile {
        version: "1.0".into(),
        kind: "hub".into(),
        backend: "docker".into(),
        config,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{path} is not a readable hub profile: {source}")]
    Malformed {
        path: String,
        #[source]
        source: serde_norway::Error,
    },
    #[error("{path} describes a {found} deployment, not a hub one")]
    WrongKind { path: String, found: String },
}

pub fn read_profile(dir: &Path) -> Result<Profile, ProfileError> {
    let path = profile_path(dir);
    let text = std::fs::read_to_string(&path)?;
    let profile: Profile =
        serde_norway::from_str(&text).map_err(|source| ProfileError::Malformed {
            path: path.to_string_lossy().to_string(),
            source,
        })?;

    if profile.kind != "hub" {
        return Err(ProfileError::WrongKind {
            path: path.to_string_lossy().to_string(),
            found: profile.kind,
        });
    }
    Ok(profile)
}

pub fn write_profile(dir: &Path, profile: &Profile) -> Result<(), ProfileError> {
    let text = serde_norway::to_string(profile).expect("a profile always serializes");
    std::fs::write(profile_path(dir), text)?;
    Ok(())
}

/// Whether a directory already holds a hub deployment.
pub fn holds_a_hub(dir: &Path) -> bool {
    profile_path(dir).exists()
}
