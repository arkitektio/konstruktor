//! Removing a deployment completely: its containers, its data, its folder, its entry.
//!
//! The other destructive paths each leave something standing on purpose — `down` keeps
//! the data, [`purge_data`] keeps the folder and the configuration, forgetting keeps
//! everything and only stops listing it. [`delete`] is the answer to "make it as if this
//! hub had never been created", and the reason both live in the core rather than in a
//! front end is that each is a sequence with an order that matters, not a single call.
//!
//! Note what `docker compose down --volumes` is *not*: with the shipped profile the
//! database and object storage are bind mounts inside the deployment folder and the stack
//! declares no named volumes, so that command removes no data at all. Deleting data is
//! [`purge_data`]'s job, and only [`purge_data`]'s.
//!
//! What it deliberately does **not** touch:
//!
//! * **Images.** `docker compose down --rmi local` would take them, but images are shared
//!   between hubs and expensive to fetch again; removing them would slow down every other
//!   deployment on the machine to tidy up after one.
//! * **The coordination server.** An authorized hub holds an identifier on a server this
//!   machine does not own. Deleting the folder cannot revoke it, and pretending otherwise
//!   would be the more dangerous lie — [`DeletionPlan::was_authorized`] exists so the
//!   caller can say so.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::compose;
use crate::config::hub::HubConfig;
use crate::profile;
use crate::reclaim::{self, SkippedMount};
use crate::registry::{self, DeploymentRecord};

#[derive(Debug, thiserror::Error)]
pub enum DeleteError {
    #[error("Konstruktor does not know a deployment with this id")]
    UnknownDeployment,
    #[error("The deployment folder could not be resolved: {0}")]
    Unresolvable(String),
    #[error(
        "Refusing to delete `{0}`: that is not a deployment folder. \
         Remove the deployment from the list instead."
    )]
    NotADeployment(String),
    #[error(
        "Refusing to delete `{0}`: it is a home or root directory. \
         A deployment folder is never one of those."
    )]
    ProtectedDirectory(String),
    #[error(
        "Docker could not take the stack down, so nothing was deleted — the containers \
         and volumes would have been left with no folder to remove them from. {0}"
    )]
    ComposeFailed(String),
    #[error("The stack was taken down, but the folder could not be removed: {0}")]
    FolderNotRemoved(String),
    #[error("The stack was taken down, but this hub's data could not be removed: {0}")]
    DataNotRemoved(String),
    #[error("This hub's configuration could not be read, so its data could not be found: {0}")]
    ProfileUnreadable(String),
    #[error("Everything was removed, but the deployment list could not be saved: {0}")]
    RegistryNotSaved(String),
}

/// What deleting one deployment would take with it, worked out before anything is done.
#[derive(Debug, Clone, Serialize)]
pub struct DeletionPlan {
    /// The folder that will be removed, canonicalized.
    pub path: String,
    /// The hub's name, which is what the user is asked to type back.
    pub name: String,
    /// Source checkouts under `mounts/`, which may hold work that exists nowhere else.
    pub checkouts: Vec<String>,
    /// The hub holds an identifier on a coordination server that this cannot revoke.
    pub was_authorized: bool,
    /// The data directories a purge would remove, resolved. Named rather than guessed at
    /// by the UI: `db_data` and `minio_data` are defaults, not constants, and a profile
    /// in the wild can point somewhere else entirely.
    pub data_dirs: Vec<String>,
    /// Mounts neither a purge nor a delete will follow, and why.
    pub skipped: Vec<SkippedMount>,
    /// The hub is on a mesh. Its tailnet state is a named volume, so `down --volumes`
    /// destroys it — and the key that joined the mesh was single-use, so the hub cannot
    /// simply come back. Worth saying before either destructive action, not after.
    pub on_a_mesh: bool,
}

/// What actually happened, step by step.
///
/// A bare `Result` would answer "did it work"; the question a user asks after a failed
/// delete is "what is still on my machine", and only a per-step account answers that.
#[derive(Debug, Clone, Serialize)]
pub struct Deletion {
    pub path: String,
    /// The containers, networks and volumes are gone.
    pub stack_removed: bool,
    /// The folder and everything in it is gone.
    pub folder_removed: bool,
    /// Konstruktor no longer lists it.
    pub forgotten: bool,
}

/// Whether a path is shaped like something we are allowed to delete recursively.
///
/// Pure, and separate from the filesystem checks, so the cases that matter can be tested
/// without building a directory tree for each one. `home` is passed rather than looked up
/// for the same reason.
///
/// The depth rule is the blunt one that catches what the named checks miss: every real
/// deployment folder is at least two levels below the root (`/home/someone/MyHub`), so
/// refusing anything shallower costs nothing and rules out `/`, `/home` and `C:\`.
pub fn check_shape(dir: &Path, home: Option<&Path>) -> Result<(), DeleteError> {
    let shown = dir.display().to_string();

    if let Some(home) = home {
        if dir == home {
            return Err(DeleteError::ProtectedDirectory(shown));
        }
    }

    let depth = dir
        .components()
        .filter(|c| matches!(c, std::path::Component::Normal(_)))
        .count();
    if depth < 2 {
        return Err(DeleteError::ProtectedDirectory(shown));
    }

    Ok(())
}

/// The checkouts a dev hub has under `mounts/`, if any.
///
/// Named rather than counted: "this also deletes your source checkouts" is a different
/// warning from "this deletes a folder", and the user should see which ones.
fn checkouts(dir: &Path) -> Vec<String> {
    let mounts = dir.join("mounts");
    let Ok(entries) = std::fs::read_dir(&mounts) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    found.sort();
    found
}

/// Resolves and validates the folder a record points at, touching nothing.
///
/// Every guard is here rather than at the call site, and the caller is given an id rather
/// than being trusted with a path: the front end cannot ask for an arbitrary directory to
/// be removed, only for a deployment it can already see to be deleted.
pub fn plan(record: &DeploymentRecord) -> Result<(PathBuf, DeletionPlan), DeleteError> {
    let dir = std::fs::canonicalize(&record.path)
        .map_err(|e| DeleteError::Unresolvable(format!("{}: {e}", record.path)))?;

    check_shape(&dir, dirs::home_dir().as_deref())?;

    // The one check that says "this is ours". A registry entry pointing somewhere that no
    // longer holds a profile is a stale entry, not a licence to delete that folder.
    if !profile::holds_a_hub(&dir) {
        return Err(DeleteError::NotADeployment(dir.display().to_string()));
    }

    // A profile that will not parse costs the preview its data directories, not the whole
    // plan: `delete` removes the folder wholesale and does not need them.
    let profile = profile::read_profile(&dir).ok();
    let found = profile
        .as_ref()
        .map(|profile| reclaim::data_dirs(&dir, &profile.config))
        .unwrap_or_default();
    let on_a_mesh = profile
        .as_ref()
        .and_then(|profile| profile.config.mesh.as_ref())
        .is_some_and(|mesh| mesh.enabled);

    let plan = DeletionPlan {
        path: dir.display().to_string(),
        name: record.name.clone(),
        checkouts: checkouts(&dir),
        was_authorized: record.identifier.is_some(),
        data_dirs: found
            .removable
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        skipped: found.skipped,
        on_a_mesh,
    };
    Ok((dir, plan))
}

/// Takes the stack down, including its volumes and anything left over from an earlier
/// shape of the compose file.
///
/// Blocking, and its output is kept: it is the only thing that explains a failure, and a
/// failure here stops the delete.
fn compose_down(dir: &Path) -> Result<(), DeleteError> {
    let output = Command::new("docker")
        .args(compose::down_everything())
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| DeleteError::ComposeFailed(format!("Could not run docker: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        return Err(DeleteError::ComposeFailed(if detail.is_empty() {
            "docker compose exited with an error.".to_string()
        } else {
            detail.to_string()
        }));
    }
    Ok(())
}

/// Deletes one deployment, entirely.
///
/// The order is the whole point:
///
/// 1. compose down, with volumes — it reads the compose file out of the folder, so it can
///    only run while the folder is still there;
/// 2. the folder;
/// 3. the registry entry.
///
/// A failure at step 1 **aborts**, leaving everything exactly as it was. Deleting the
/// folder while the containers are still up is the one mistake here that cannot be undone
/// from inside the app — the stack would keep running with nothing left to stop it by.
/// Being unable to delete a hub while Docker is off is merely inconvenient.
pub fn delete(id: &str) -> Result<Deletion, DeleteError> {
    let mut store = registry::load();
    let record = store
        .deployments
        .iter()
        .find(|d| d.id == id)
        .ok_or(DeleteError::UnknownDeployment)?
        .clone();

    let (dir, _) = plan(&record)?;

    // Read before the stack goes down and long before the folder does: the images are in
    // the profile, and the profile is inside the folder we are about to remove. A profile
    // that will not parse is not fatal here — the delete simply loses its ability to
    // repair ownership, and says so if it then hits one.
    let images = profile::read_profile(&dir)
        .map(|profile| reclaim::repair_images(&profile.config))
        .unwrap_or_default();

    compose_down(&dir)?;

    // The whole folder is going, so handing all of it back to its owner is proportionate.
    // If the retry still fails, a dev hub's `mounts/` checkouts have been reowned to the
    // desktop user — harmless, since they were the user's to begin with, but the error
    // says so rather than leaving it to be discovered.
    reclaim::remove_tree(&dir, &dir, &images)
        .map_err(|e| DeleteError::FolderNotRemoved(e.to_string()))?;

    store.deployments.retain(|d| d.id != id);
    registry::save(&store).map_err(|e| DeleteError::RegistryNotSaved(e.to_string()))?;

    Ok(Deletion {
        path: dir.display().to_string(),
        stack_removed: true,
        folder_removed: true,
        forgotten: true,
    })
}

/// What a data purge removed, and what it deliberately did not.
#[derive(Debug, Clone, Serialize)]
pub struct DataPurge {
    pub path: String,
    /// The containers and networks are gone; the stack has to be started again.
    pub stack_removed: bool,
    /// The data directories that are gone.
    pub removed: Vec<String>,
    /// Mounts left alone, and why — an absolute or escaping mount in a hand-edited
    /// profile is reported, never followed.
    pub skipped: Vec<SkippedMount>,
}

/// Removes the resolved data directories, and reports what happened to each.
///
/// Split out from [`purge_data`] so it can be tested: everything above it shells out to
/// Docker, and this is the half that decides what is deleted.
///
/// A failure on one directory does not stop the others. The question a user asks after a
/// failed purge is "what is still on my machine", and only finishing the job and then
/// reporting per-directory answers it.
fn purge_dirs(dir: &Path, config: &HubConfig) -> (Vec<String>, Vec<String>) {
    let found = reclaim::data_dirs(dir, config);
    let images = reclaim::repair_images(config);

    let mut removed = Vec::new();
    let mut failures = Vec::new();
    for target in found.removable {
        match reclaim::remove_tree(&target, dir, &images) {
            Ok(()) => removed.push(target.display().to_string()),
            Err(e) => failures.push(e.to_string()),
        }
    }
    (removed, failures)
}

/// Deletes a hub's data, and leaves the hub.
///
/// Keyed by id like [`delete`], so it goes through the same [`plan`] guards and no caller
/// can name a directory to be removed. What survives is everything that is not data: the
/// folder, `hub_config.yaml`, the credentials, `docker-compose.yaml`, `configs/` and a dev
/// hub's `mounts/` — by construction, since the only paths removed are the ones
/// `reclaim::resolve_mount` handed back.
///
/// The containers come down first and that is not optional: the data directories cannot
/// be removed from under a running Postgres.
///
/// The directories are **resolved before** the stack is taken down, so a profile that
/// names something we will not touch is reported without having stopped the hub for
/// nothing.
pub fn purge_data(id: &str) -> Result<DataPurge, DeleteError> {
    let store = registry::load();
    let record = store
        .deployments
        .iter()
        .find(|d| d.id == id)
        .ok_or(DeleteError::UnknownDeployment)?
        .clone();

    let (dir, _) = plan(&record)?;

    // No fallback to hardcoded `db_data` / `minio_data` if this fails. Guessing which
    // directories to remove recursively is exactly what the guards exist to prevent.
    let profile =
        profile::read_profile(&dir).map_err(|e| DeleteError::ProfileUnreadable(e.to_string()))?;
    let config = profile.config;

    let skipped = reclaim::data_dirs(&dir, &config).skipped;

    compose_down(&dir)?;

    let (removed, failures) = purge_dirs(&dir, &config);
    if !failures.is_empty() {
        return Err(DeleteError::DataNotRemoved(failures.join("; ")));
    }

    Ok(DataPurge {
        path: dir.display().to_string(),
        stack_removed: true,
        removed,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_a_home_directory() {
        let home = Path::new("/home/someone");
        assert!(matches!(
            check_shape(home, Some(home)),
            Err(DeleteError::ProtectedDirectory(_))
        ));
    }

    #[test]
    fn refuses_the_root_and_everything_directly_under_it() {
        for shallow in ["/", "/home", "/opt"] {
            assert!(
                matches!(
                    check_shape(Path::new(shallow), None),
                    Err(DeleteError::ProtectedDirectory(_))
                ),
                "{shallow} should have been refused"
            );
        }
    }

    #[test]
    fn accepts_a_folder_two_levels_down() {
        let home = Path::new("/home/someone");
        assert!(check_shape(Path::new("/home/someone/MyHub"), Some(home)).is_ok());
    }

    /// A scratch directory of our own, since there is no `tempfile` here and this needs a
    /// real path for `canonicalize` and `holds_a_hub` to have anything to say.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "konstruktor-destroy-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    fn record_at(dir: &Path) -> DeploymentRecord {
        DeploymentRecord {
            id: "abc".into(),
            name: "MyHub".into(),
            path: dir.display().to_string(),
            kind: "hub".into(),
            project: "myhub".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            last_generated_at: None,
            coord_server: None,
            identifier: None,
        }
    }

    /// The guard that matters most: a registry entry can outlive the folder it names, or
    /// name one the user has since put something else in. Neither is a licence to delete
    /// it recursively.
    #[test]
    fn refuses_a_folder_that_holds_no_hub() {
        let dir = scratch("no-hub");
        std::fs::write(dir.join("holiday-photos.txt"), "not a hub").unwrap();

        let outcome = plan(&record_at(&dir));
        assert!(
            matches!(outcome, Err(DeleteError::NotADeployment(_))),
            "expected a refusal, got {outcome:?}"
        );
        assert!(dir.exists(), "planning must never remove anything");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn plans_a_real_deployment_folder_without_touching_it() {
        let dir = scratch("real-hub");
        std::fs::write(dir.join(crate::profile::HUB_CONFIG_FILENAME), "{}").unwrap();
        std::fs::create_dir_all(dir.join("mounts").join("rekuest")).unwrap();
        std::fs::create_dir_all(dir.join("mounts").join("mikro")).unwrap();

        let mut record = record_at(&dir);
        record.identifier = Some("mylab".into());

        let (resolved, plan) = plan(&record).expect("a real hub folder plans");
        assert_eq!(resolved, std::fs::canonicalize(&dir).unwrap());
        assert_eq!(plan.name, "MyHub");
        // Named, so the confirmation can say which checkouts go with the folder.
        assert_eq!(
            plan.checkouts,
            vec!["mikro".to_string(), "rekuest".to_string()]
        );
        assert!(plan.was_authorized);
        assert!(dir.exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The half of the purge that decides what is deleted, over a folder laid out like a
    /// real deployment. Everything that is not data has to survive it — this is the test
    /// that would catch a purge that started eating the configuration or a dev hub's
    /// checkouts.
    #[test]
    fn purges_the_data_and_nothing_else() {
        use crate::config::hub::{build_hub_config, HubConfigOptions};

        let dir = scratch("purge");
        for data in ["db_data", "minio_data"] {
            std::fs::create_dir_all(dir.join(data)).unwrap();
            std::fs::write(dir.join(data).join("some.db"), "rows").unwrap();
        }
        std::fs::create_dir_all(dir.join("configs")).unwrap();
        std::fs::write(dir.join("configs").join("Caddyfile"), "caddy").unwrap();
        std::fs::create_dir_all(dir.join("mounts").join("rekuest")).unwrap();
        std::fs::write(dir.join("mounts").join("rekuest").join("main.py"), "src").unwrap();
        std::fs::write(dir.join(crate::profile::HUB_CONFIG_FILENAME), "{}").unwrap();
        std::fs::write(dir.join("hub_credentials.json"), "{}").unwrap();
        std::fs::write(dir.join("docker-compose.yaml"), "services: {}").unwrap();

        let config = build_hub_config(&HubConfigOptions::default());
        let (removed, failures) = purge_dirs(&dir, &config);

        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(removed.len(), 2);
        assert!(!dir.join("db_data").exists());
        assert!(!dir.join("minio_data").exists());

        for survivor in [
            crate::profile::HUB_CONFIG_FILENAME,
            "hub_credentials.json",
            "docker-compose.yaml",
        ] {
            assert!(
                dir.join(survivor).exists(),
                "{survivor} must survive a purge"
            );
        }
        assert!(dir.join("configs").join("Caddyfile").exists());
        assert!(
            dir.join("mounts").join("rekuest").join("main.py").exists(),
            "a dev hub's checkouts are not data and must survive"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The home check is an exact match, not a prefix: `MyHub` living *inside* the home
    /// directory is the normal case, and a folder whose name merely starts the same way
    /// is somebody else's business but still shaped like a deployment.
    #[test]
    fn only_the_home_directory_itself_is_protected() {
        let home = Path::new("/home/someone");
        assert!(check_shape(Path::new("/home/someone/MyHub"), Some(home)).is_ok());
        assert!(check_shape(Path::new("/home/someone-else"), Some(home)).is_ok());
        assert!(check_shape(Path::new("/srv/hubs/MyHub"), Some(home)).is_ok());
    }
}
