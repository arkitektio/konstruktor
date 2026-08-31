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

/// Points services at different images, and regenerates the deployment from the result.
///
/// The only way to move a running container onto another image: generation reads
/// `config.<service>.image` out of the profile, so nothing changes until the profile does.
/// Two callers need it — `rollback`, putting older images back, and `update --infra`,
/// advancing a pin — and they must not each grow their own copy of the sequence.
///
/// Generation happens before anything is written, as everywhere else that touches a
/// deployment folder: a profile this build cannot generate from has to leave the folder
/// unchanged rather than half rewritten.
pub fn rewrite_images(dir: &Path, images: &[(String, String)]) -> Result<(), ProfileError> {
    let mut config = read_profile(dir)?.config;
    for (service, image) in images {
        config.set_service_image(service, image);
    }

    // The identity the last authorization issued. Absent on a hub that was never
    // authorized, where the default is what generation already used.
    let identity = crate::credentials::read_credentials(dir)
        .map(|credentials| credentials.issued_identity())
        .unwrap_or_default();
    let files = crate::generate::generate_hub_files(&config, &identity);

    write_profile(dir, &hub_profile(config))?;
    crate::generate::write::write_generated_files(dir, &files)?;
    Ok(())
}

/// Whether a directory already holds a hub deployment.
pub fn holds_a_hub(dir: &Path) -> bool {
    profile_path(dir).exists()
}

/// The three shapes a deployment folder comes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentKind {
    Hub,
    Engine,
    /// A coordination server — the thing the other two authorize against.
    Coord,
}

impl DeploymentKind {
    pub fn label(self) -> &'static str {
        match self {
            DeploymentKind::Hub => "hub",
            DeploymentKind::Engine => "plugin engine",
            DeploymentKind::Coord => "coordination server",
        }
    }

    /// The registry's `kind` string, which predates this enum and is what is on disk.
    pub fn as_kind(self) -> &'static str {
        match self {
            DeploymentKind::Hub => "hub",
            DeploymentKind::Engine => "engine",
            DeploymentKind::Coord => crate::coord::COORD_KIND,
        }
    }
}

/// What kind of deployment a folder holds, if it holds one at all.
///
/// A hub is recognised by its profile. Neither of the other two has one — a plugin engine
/// is a single deployer container and a coordination server runs Lok — so each is known by
/// its own config file, and a bare compose project with neither is taken for an engine,
/// which is what every engine written before coordination servers existed looks like.
/// A folder with none of these is not something this ever created.
///
/// This is the rule `destroy::plan` has always applied before deleting anything; it lives
/// here so that resolving a deployment and deleting one cannot disagree about what counts.
pub fn holds_a_deployment(dir: &Path) -> Option<DeploymentKind> {
    if holds_a_hub(dir) {
        return Some(DeploymentKind::Hub);
    }
    if crate::coord::holds_a_coord(dir) {
        return Some(DeploymentKind::Coord);
    }
    if dir.join(crate::engine::CONFIG_FILE).is_file()
        || dir.join(crate::compose_file::COMPOSE_FILENAME).is_file()
    {
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
    fn a_lok_config_makes_it_a_coordination_server() {
        let dir = tmpdir();
        std::fs::create_dir_all(dir.join("configs")).unwrap();
        std::fs::write(dir.join(crate::coord::COORD_CONFIG_FILE), "lok: {}").unwrap();
        // It has a compose file too, as every deployment does. The marker has to win, or
        // a coordination server would resolve as a plugin engine.
        std::fs::write(dir.join(crate::compose_file::COMPOSE_FILENAME), "services: {}").unwrap();
        assert_eq!(holds_a_deployment(&dir), Some(DeploymentKind::Coord));
    }

    /// Engines written before coordination servers existed have only a compose file.
    #[test]
    fn a_deployer_config_and_a_bare_compose_are_both_engines() {
        let dir = tmpdir();
        std::fs::create_dir_all(dir.join("configs")).unwrap();
        std::fs::write(dir.join(crate::engine::CONFIG_FILE), "deployer: {}").unwrap();
        assert_eq!(holds_a_deployment(&dir), Some(DeploymentKind::Engine));

        let legacy = tmpdir();
        std::fs::write(legacy.join(crate::compose_file::COMPOSE_FILENAME), "services: {}").unwrap();
        assert_eq!(holds_a_deployment(&legacy), Some(DeploymentKind::Engine));
    }

    #[test]
    fn an_empty_folder_holds_nothing() {
        assert_eq!(holds_a_deployment(&tmpdir()), None);
    }
}
