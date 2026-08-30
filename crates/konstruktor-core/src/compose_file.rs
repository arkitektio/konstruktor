//! Reading and writing a deployment's `docker-compose.yaml` by hand.
//!
//! The generator writes the file once, when the hub is created, and nothing rewrites it
//! afterwards — so it is the one place a person can change how the stack runs without
//! Konstruktor's say-so: an extra volume, a port, a resource limit. Both front ends offer
//! to edit it, and the rules for doing that safely live here rather than in either.
//!
//! What is *not* here: reconciling an edited file with the profile. The profile is what
//! the generator reads; the compose file is what Docker reads; after an edit the two say
//! different things, and that is the whole point of editing it. [`regenerate`] hands
//! back what the generator would write today, for a front end that wants a "start over"
//! button, and leaves choosing between them to the person.

use std::path::{Path, PathBuf};

use crate::config::hub::HubConfig;
use crate::generate::compose::build_compose;
use crate::generate::dump;
use crate::profile;

/// The file name compose looks for first, and the one the generator writes.
pub const COMPOSE_FILENAME: &str = "docker-compose.yaml";

/// Where the previous contents go on every write, so one bad edit is never the end of
/// a hub. Overwritten on each save: one step back is what the button offers.
pub const COMPOSE_BACKUP_FILENAME: &str = "docker-compose.yaml.bak";

#[derive(Debug, thiserror::Error)]
pub enum ComposeFileError {
    #[error("{0} is not a deployment folder")]
    NotADeployment(String),
    #[error("could not read {path}: {source}")]
    Unreadable {
        path: String,
        source: std::io::Error,
    },
    #[error("could not write {path}: {source}")]
    Unwritable {
        path: String,
        source: std::io::Error,
    },
    #[error("the file is not valid YAML: {0}")]
    NotYaml(String),
    #[error("the profile could not be read: {0}")]
    Profile(String),
}

fn compose_path(dir: &Path) -> Result<PathBuf, ComposeFileError> {
    if !profile::holds_a_hub(dir) && !dir.join(COMPOSE_FILENAME).exists() {
        return Err(ComposeFileError::NotADeployment(dir.display().to_string()));
    }
    Ok(dir.join(COMPOSE_FILENAME))
}

/// The compose file as it is on disk.
pub fn read(dir: &Path) -> Result<String, ComposeFileError> {
    let path = compose_path(dir)?;
    std::fs::read_to_string(&path).map_err(|source| ComposeFileError::Unreadable {
        path: path.display().to_string(),
        source,
    })
}

/// Whether a `.bak` from an earlier save is there to go back to.
pub fn has_backup(dir: &Path) -> bool {
    dir.join(COMPOSE_BACKUP_FILENAME).exists()
}

/// The previous contents, if a save has kept one.
pub fn read_backup(dir: &Path) -> Result<Option<String>, ComposeFileError> {
    let path = dir.join(COMPOSE_BACKUP_FILENAME);
    if !path.exists() {
        return Ok(None);
    }
    std::fs::read_to_string(&path)
        .map(Some)
        .map_err(|source| ComposeFileError::Unreadable {
            path: path.display().to_string(),
            source,
        })
}

/// Writes new contents, keeping the old ones as [`COMPOSE_BACKUP_FILENAME`].
///
/// Only the *shape* is checked — that it parses as YAML with a `services` mapping — and
/// that check is here because a file that is not YAML at all would take the whole hub
/// down with it: every compose command, `stop` and `down` included, starts by parsing
/// it. Whether the contents mean anything to Docker is Docker's to say; [`validate`]
/// asks it.
pub fn write(dir: &Path, contents: &str) -> Result<(), ComposeFileError> {
    let path = compose_path(dir)?;

    let parsed: serde_norway::Value = serde_norway::from_str(contents)
        .map_err(|e| ComposeFileError::NotYaml(e.to_string()))?;
    if parsed.get("services").and_then(|s| s.as_mapping()).is_none() {
        return Err(ComposeFileError::NotYaml(
            "a compose file needs a `services:` mapping at the top level".into(),
        ));
    }

    if path.exists() {
        let backup = dir.join(COMPOSE_BACKUP_FILENAME);
        std::fs::copy(&path, &backup).map_err(|source| ComposeFileError::Unwritable {
            path: backup.display().to_string(),
            source,
        })?;
    }

    // Through a sibling and a rename, so a crash mid-write leaves the old file whole
    // rather than a truncated one nothing can parse.
    let staging = dir.join(format!("{COMPOSE_FILENAME}.tmp"));
    std::fs::write(&staging, contents)
        .and_then(|_| std::fs::rename(&staging, &path))
        .map_err(|source| ComposeFileError::Unwritable {
            path: path.display().to_string(),
            source,
        })
}

/// What the generator would write for this hub's profile today.
///
/// Not written anywhere: a front end shows it, or offers it as the thing to reset to,
/// and only [`write`] puts anything on disk.
pub fn regenerate(dir: &Path) -> Result<String, ComposeFileError> {
    let profile = profile::read_profile(dir).map_err(|e| ComposeFileError::Profile(e.to_string()))?;
    Ok(regenerate_from(&profile.config))
}

pub fn regenerate_from(config: &HubConfig) -> String {
    dump(&build_compose(config, &config.enabled_services()))
}

/// `docker compose config --quiet`: Docker's own verdict on the file, as the error text it
/// prints, or `Ok` when it has nothing to say.
///
/// Asked of the engine rather than reimplemented, because the compose specification is
/// large and moves — and because what matters is whether *this* engine accepts the file.
pub async fn validate(dir: &Path) -> Result<(), String> {
    let output = crate::engine_probe::engine()
        .async_command()
        .args(["compose", "config", "--quiet"])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "konstruktor-compose-file-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_save_keeps_the_previous_file_and_refuses_non_yaml() {
        let dir = scratch("save");
        std::fs::write(dir.join(COMPOSE_FILENAME), "services: {}\n").unwrap();

        write(&dir, "services:\n  db:\n    image: postgres\n").unwrap();
        assert_eq!(read_backup(&dir).unwrap().as_deref(), Some("services: {}\n"));
        assert!(read(&dir).unwrap().contains("postgres"));

        // Neither of these may touch the file.
        assert!(matches!(
            write(&dir, "services: [\n"),
            Err(ComposeFileError::NotYaml(_))
        ));
        assert!(matches!(
            write(&dir, "volumes: {}\n"),
            Err(ComposeFileError::NotYaml(_))
        ));
        assert!(read(&dir).unwrap().contains("postgres"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn refuses_a_folder_that_is_not_a_deployment() {
        let dir = scratch("empty");
        assert!(matches!(
            read(&dir),
            Err(ComposeFileError::NotADeployment(_))
        ));
        std::fs::remove_dir_all(&dir).ok();
    }
}
