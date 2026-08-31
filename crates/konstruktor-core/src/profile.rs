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

/// The two shapes a deployment folder comes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentKind {
    Hub,
    Engine,
}

impl DeploymentKind {
    pub fn label(self) -> &'static str {
        match self {
            DeploymentKind::Hub => "hub",
            DeploymentKind::Engine => "plugin engine",
        }
    }
}

/// What kind of deployment a folder holds, if it holds one at all.
///
/// A hub is recognised by its profile. A plugin engine has none — it is one deployer
/// container — so the compose file Konstruktor wrote is what stands in: a folder with
/// neither is not something this ever created.
///
/// This is the rule `destroy::plan` has always applied before deleting anything; it lives
/// here so that resolving a deployment and deleting one cannot disagree about what counts.
pub fn holds_a_deployment(dir: &Path) -> Option<DeploymentKind> {
    if holds_a_hub(dir) {
        return Some(DeploymentKind::Hub);
    }
    if dir.join(crate::compose_file::COMPOSE_FILENAME).is_file() {
        return Some(DeploymentKind::Engine);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("konstruktor-profile-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_profile_makes_it_a_hub() {
        let dir = tmpdir();
        std::fs::write(profile_path(&dir), "version: '1.0'").unwrap();
        assert_eq!(holds_a_deployment(&dir), Some(DeploymentKind::Hub));
    }

    #[test]
    fn a_bare_compose_file_makes_it_an_engine() {
        let dir = tmpdir();
        std::fs::write(dir.join(crate::compose_file::COMPOSE_FILENAME), "services: {}").unwrap();
        assert_eq!(holds_a_deployment(&dir), Some(DeploymentKind::Engine));
    }

    /// A hub also has a compose file. The profile has to win, or every hub would resolve
    /// as an engine and lose the commands that need its config.
    #[test]
    fn a_hub_with_a_compose_file_is_still_a_hub() {
        let dir = tmpdir();
        std::fs::write(profile_path(&dir), "version: '1.0'").unwrap();
        std::fs::write(dir.join(crate::compose_file::COMPOSE_FILENAME), "services: {}").unwrap();
        assert_eq!(holds_a_deployment(&dir), Some(DeploymentKind::Hub));
    }

    #[test]
    fn an_empty_folder_holds_nothing() {
        assert_eq!(holds_a_deployment(&tmpdir()), None);
    }
}
