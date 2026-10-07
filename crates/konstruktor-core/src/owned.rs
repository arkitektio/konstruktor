//! What in a deployment folder is Konstruktor's to delete.
//!
//! A deployment folder is whatever folder somebody pointed Konstruktor at, and that can be
//! a home directory with a life of its own. So a delete never removes "the folder": it
//! removes the things named here, one by one, and whatever else is in the folder is not
//! looked at, let alone touched. [`of`] says which things those are and [`remove`] takes
//! them away; the folder itself goes only when that left it empty, and only when the
//! caller says it is a folder that may go at all.
//!
//! Two kinds of thing are named, and the difference is the point:
//!
//! * **Files**, by their exact path. `configs/`, `secrets/`, `facts/` and `overrides/` are
//!   names anybody might have used, so they are never removed as directories: the files
//!   Konstruktor wrote into them are, and a directory goes afterwards only if it is empty.
//! * **Trees**, removed whole, and only these: Konstruktor's own `.konstruktor/` and
//!   rescue folder, the data directories the profile names (`reclaim::data_dirs`, with all
//!   its guards), and the checkouts it cloned under `mounts/` — the ones the lock says it
//!   cloned, not whatever is there: a checkout that was in place before the hub was made
//!   is used as it is and stays as it is.
//!   Where such a checkout is a link to a tree kept elsewhere — the Python package puts
//!   one there for `mounts=` — the link goes and the tree is not followed.
//!
//! The list is built from what was written, never from a pattern: the lock's record of the
//! generated files, what the generator would write for this profile today, and the fixed
//! names each kind of deployment has. A file of Konstruktor's that none of these names is
//! left behind and reported, which is the right way round for a delete to be wrong.
//!
//! Paths out of the lock and the profile are strings out of files a person can edit. They
//! are resolved the way `reclaim::resolve_mount` resolves a mount: nothing absolute,
//! nothing with `..`, and nothing reached through a symbolic link.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::compose_file::{COMPOSE_BACKUP_FILENAME, COMPOSE_FILENAME};
use crate::config::hub::{HubConfig, RunsFrom};
use crate::profile::{DeploymentKind, HUB_CONFIG_FILENAME};

/// The files that make a folder a deployment (`profile::holds_a_deployment`), and the
/// lock, which says what else in it is ours. Removed last, and only once everything else
/// is gone: a delete that failed half-way has to still find a deployment here when it is
/// asked again, and still know what to take.
const MARKERS: &[&str] = &[
    HUB_CONFIG_FILENAME,
    crate::lock::LOCK_FILENAME,
    COMPOSE_FILENAME,
    crate::engine::CONFIG_FILE,
    crate::coord::COORD_CONFIG_FILE,
];

/// Which app was handed which of a hub's redeem tokens: written beside the access
/// document by the Python package (`TAKEN_FILE` in `python/konstruktor/hub.py`), which is
/// the only thing in a hub's folder that the binary did not write itself.
const TAKEN_FILE: &str = "secrets/redeem-tokens-taken.json";

/// The folders that are Konstruktor's own by name, whatever kind of deployment this is.
const TREES: &[&str] = &[".konstruktor", crate::deregister::RESCUE_DIR];

/// What a delete removes from one deployment folder.
#[derive(Debug, Clone, Default)]
pub struct Owned {
    /// Files, by their path in the folder: relative and POSIX-separated.
    pub files: BTreeSet<String>,
    /// Directories removed with everything in them, resolved.
    pub trees: Vec<PathBuf>,
    /// Links standing where a checkout would be, removed as links: what they point at is
    /// somebody's own tree, used where it is, and is not followed.
    pub links: Vec<PathBuf>,
}

/// What became of a folder once Konstruktor's things were taken out of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cleared {
    /// The folder was empty afterwards, was allowed to go, and is gone.
    pub folder_removed: bool,
    /// What the folder still holds, by name, when it could have gone and did not.
    pub left_behind: Vec<String>,
}

/// Where a path from the lock or the profile lands in the folder, if it is there and is
/// reached without leaving the folder or following a link.
fn resolve(dir: &Path, relative: &str) -> Option<PathBuf> {
    let raw = relative.trim();
    let mut chars = raw.chars();
    let first = chars.next()?;
    // Absolute on any platform, by shape: see `reclaim::resolve_mount`.
    if first == '/' || first == '\\' || first == '~' {
        return None;
    }
    if first.is_ascii_alphabetic() && chars.next() == Some(':') {
        return None;
    }

    let mut resolved = dir.to_path_buf();
    let mut any = false;
    for segment in raw.split(['/', '\\']) {
        match segment {
            "" | "." => continue,
            ".." => return None,
            other => resolved.push(other),
        }
        any = true;
        // Every component, not only the last: a linked `configs/` leads somewhere else.
        let metadata = std::fs::symlink_metadata(&resolved).ok()?;
        if metadata.file_type().is_symlink() {
            return None;
        }
    }
    any.then_some(resolved)
}

/// The lock, read and nothing more: `lock::read` moves an unreadable one aside, and
/// working out what a delete would take must not change the folder.
fn recorded(dir: &Path) -> crate::lock::Lock {
    std::fs::read_to_string(crate::lock::lock_path(dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Everything in `dir` that Konstruktor wrote, for a deployment of this kind.
///
/// `config` is the hub's profile when it could be read. Without one the list is the fixed
/// names and what the lock recorded — shorter, and what is missing from it stays.
pub fn of(dir: &Path, kind: DeploymentKind, config: Option<&HubConfig>) -> Owned {
    let mut files: BTreeSet<String> = [
        COMPOSE_FILENAME.to_string(),
        COMPOSE_BACKUP_FILENAME.to_string(),
        format!("{COMPOSE_FILENAME}.tmp"),
    ]
    .into();
    let mut trees: Vec<PathBuf> = Vec::new();
    let mut links: Vec<PathBuf> = Vec::new();

    match kind {
        DeploymentKind::Engine => {
            files.insert(crate::engine::CONFIG_FILE.to_string());
            files.insert(crate::engine::MESH_FILE.to_string());
        }
        DeploymentKind::Coord => {
            files.insert(crate::coord::COORD_CONFIG_FILE.to_string());
        }
        DeploymentKind::Hub => {
            let lock = crate::lock::LOCK_FILENAME;
            let state = crate::hubhealth::STATE_FILENAME;
            files.extend([
                HUB_CONFIG_FILENAME.to_string(),
                lock.to_string(),
                format!("{lock}{}", crate::lock::QUARANTINE_SUFFIX),
                crate::credentials::CREDENTIALS_FILENAME.to_string(),
                crate::deregister::DEREGISTERED_FILENAME.to_string(),
                state.to_string(),
                format!("{state}.partial"),
                crate::config::mesh::MESH_ENV_FILE.to_string(),
                TAKEN_FILE.to_string(),
            ]);

            let held = recorded(dir);
            // A service's config, the facts it was written from and what the operator
            // set for it: by the services the record knows, and below by the profile's.
            let mut hosts: BTreeSet<String> = held.rendered.keys().cloned().collect();
            hosts.extend(held.described.keys().cloned());
            files.extend(held.files.into_keys());
            // Only the checkouts written down as cloned. A hub made before they were
            // written down keeps all of its checkouts, which is the way to be wrong.
            let mounts = crate::generate::compose::MOUNTS_DIR;
            for host in &held.checkouts {
                trees.extend(resolve(dir, &format!("{mounts}/{host}")));
            }
            let said: crate::contract::Said = held
                .described
                .into_iter()
                .filter_map(|(host, known)| Some((host, known.description?)))
                .collect();

            if let Some(config) = config {
                files.extend(
                    crate::generate::generate_hub_files(config, &Default::default(), &said)
                        .into_keys(),
                );
                for id in config.enabled_services() {
                    let service = config.service(id);
                    hosts.insert(service.host.clone());
                    // A tree kept elsewhere and linked in where the checkout would be.
                    if service.mount_github
                        && matches!(service.runs_from(), Some(RunsFrom::Repository { .. }))
                    {
                        links.extend(link_in(dir, mounts, &service.host));
                    }
                }
                trees.extend(crate::reclaim::data_dirs(dir, config).removable);
            }

            for host in hosts {
                files.insert(format!("configs/{host}.yaml"));
                files.insert(format!("{}/{host}.yaml", crate::contract::FACTS_DIR));
                files.insert(format!("{}/{host}.yaml", crate::overrides::OVERRIDES_DIR));
            }
        }
    }

    trees.extend(TREES.iter().filter_map(|name| resolve(dir, name)));
    trees.retain(|tree| tree.is_dir());
    trees.sort();
    trees.dedup();

    Owned {
        files,
        trees,
        links,
    }
}

/// `folder/name` when it is a symbolic link and `folder` itself is reached without one.
fn link_in(dir: &Path, folder: &str, name: &str) -> Option<PathBuf> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return None;
    }
    let link = resolve(dir, folder)?.join(name);
    std::fs::symlink_metadata(&link)
        .ok()?
        .file_type()
        .is_symlink()
        .then_some(link)
}

impl Owned {
    /// What of this is on disk now, as paths: what a delete is about to take.
    pub fn present(&self, dir: &Path) -> Vec<String> {
        self.trees
            .iter()
            .chain(&self.links)
            .cloned()
            .chain(self.files.iter().filter_map(|file| resolve(dir, file)))
            .map(|path| path.display().to_string())
            .collect()
    }

    /// The checkouts among the trees, by service: named rather than counted, because
    /// "this also deletes your source checkouts" is a different warning from "this
    /// deletes a hub", and the user should see which ones.
    pub fn checkouts(&self, dir: &Path) -> Vec<String> {
        let mounts = dir.join(crate::generate::compose::MOUNTS_DIR);
        self.trees
            .iter()
            .filter(|tree| tree.parent() == Some(mounts.as_path()))
            .filter_map(|tree| Some(tree.file_name()?.to_string_lossy().to_string()))
            .collect()
    }

    /// The directories these things are in, deepest first: the ones that may be empty,
    /// and Konstruktor's to remove, once the things themselves are gone.
    fn parents(&self, dir: &Path) -> Vec<PathBuf> {
        let inside = self
            .trees
            .iter()
            .chain(&self.links)
            .filter_map(|tree| tree.strip_prefix(dir).ok().map(Path::to_path_buf))
            .chain(self.files.iter().map(PathBuf::from));
        let mut parents: BTreeSet<PathBuf> = BTreeSet::new();
        for path in inside {
            let mut at = path.parent();
            while let Some(parent) = at.filter(|p| !p.as_os_str().is_empty()) {
                parents.insert(parent.to_path_buf());
                at = parent.parent();
            }
        }
        let mut parents: Vec<PathBuf> = parents.into_iter().collect();
        parents.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        parents
    }
}

/// Removes one file of ours. `Ok` for one that is not there, or that a link stands in
/// front of — neither is a failure, and the second is simply left.
fn remove_file(dir: &Path, relative: &str) -> Result<(), String> {
    let Some(path) = resolve(dir, relative) else {
        return Ok(());
    };
    // Docker makes a directory of a bind mount whose file was missing. An empty one is
    // that, and goes; one with something in it is not ours to empty.
    let removed = if path.is_dir() {
        let _ = std::fs::remove_dir(&path);
        Ok(())
    } else {
        std::fs::remove_file(&path)
    };
    match removed {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("{}: {error}", path.display()))
        }
        _ => Ok(()),
    }
}

/// Removes what [`of`] named from `dir`, and then `dir` itself if it `may_go` and that
/// left it empty.
///
/// Nothing here is recursive except the trees, and nothing is removed that was not named:
/// a directory is taken with the non-recursive `remove_dir`, which refuses one that still
/// holds something, and that refusal is the guard. Ownership is repaired
/// (`reclaim::remove_tree`) tree by tree and never for the folder — a folder that is
/// somebody's home is not handed to anybody.
///
/// A failure does not stop the rest, and the files that make this a deployment go only
/// after everything else did, so that a delete which could not finish can be asked again.
pub fn remove(
    dir: &Path,
    owned: &Owned,
    may_go: bool,
    images: &[String],
) -> Result<Cleared, String> {
    let mut failures: Vec<String> = Vec::new();

    for tree in &owned.trees {
        if let Err(error) = crate::reclaim::remove_tree(tree, dir, images) {
            failures.push(error.to_string());
        }
    }
    for link in &owned.links {
        // The link itself, by either name the platform has for removing one.
        if let Err(error) = std::fs::remove_file(link).or_else(|_| std::fs::remove_dir(link)) {
            failures.push(format!("{}: {error}", link.display()));
        }
    }
    let (markers, rest): (Vec<&String>, Vec<&String>) = owned
        .files
        .iter()
        .partition(|file| MARKERS.contains(&file.as_str()));
    for file in rest {
        failures.extend(remove_file(dir, file).err());
    }
    if !failures.is_empty() {
        return Err(failures.join("; "));
    }
    for file in markers {
        failures.extend(remove_file(dir, file).err());
    }
    if !failures.is_empty() {
        return Err(failures.join("; "));
    }

    for parent in owned.parents(dir) {
        // Refused for a directory that still holds something, which is then not ours.
        if let Some(path) = resolve(dir, &parent.to_string_lossy()) {
            let _ = std::fs::remove_dir(path);
        }
    }

    if !may_go {
        return Ok(Cleared::default());
    }
    if std::fs::remove_dir(dir).is_ok() {
        return Ok(Cleared {
            folder_removed: true,
            left_behind: Vec::new(),
        });
    }
    let mut left_behind: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    left_behind.sort();
    Ok(Cleared {
        folder_removed: false,
        left_behind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::hub::{build_hub_config, HubConfigOptions, StorageMode};

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "konstruktor-owned-{tag}-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        std::fs::canonicalize(&dir).expect("a scratch directory resolves")
    }

    fn put(dir: &Path, relative: &str) {
        let target = dir.join(relative);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, "x").unwrap();
    }

    /// A hub in folder mode, laid out the way one that has run is.
    fn a_hub(dir: &Path) -> HubConfig {
        let config = build_hub_config(&HubConfigOptions {
            storage: StorageMode::DeploymentFolder,
            ..Default::default()
        });
        let host = config.service(config.enabled_services()[0]).host.clone();
        for file in [
            HUB_CONFIG_FILENAME.to_string(),
            COMPOSE_FILENAME.to_string(),
            COMPOSE_BACKUP_FILENAME.to_string(),
            "hub_credentials.json".to_string(),
            "hub_lock.json".to_string(),
            "reporter.json".to_string(),
            format!("configs/{host}.yaml"),
            format!("facts/{host}.yaml"),
            format!("overrides/{host}.yaml"),
            "db_data/base/1".to_string(),
            "rustfs_data/bucket/object".to_string(),
            ".konstruktor/generations/1/generation.json".to_string(),
        ] {
            put(dir, &file);
        }
        // What the lock recorded is ours too, whatever the generator writes today.
        std::fs::write(
            dir.join("hub_lock.json"),
            r#"{"version":1,"files":{"configs/Caddyfile":"00","secrets/kept.key":"00"}}"#,
        )
        .unwrap();
        put(dir, "configs/Caddyfile");
        put(dir, "secrets/kept.key");
        config
    }

    /// The case this module exists for: a hub made in a home directory. Everything
    /// Konstruktor wrote goes, everything else stays exactly where it was — including
    /// files in the directories whose names Konstruktor also uses — and so does the home.
    #[test]
    fn a_hub_in_a_home_directory_loses_only_what_konstruktor_wrote() {
        let home = scratch("home");
        let config = a_hub(&home);
        for mine in [
            "holiday-photos.txt",
            ".bashrc",
            "configs/my-editor.yaml",
            "secrets/my-own.key",
            "mounts/somebody-elses/main.py",
            "Documents/thesis.tex",
        ] {
            put(&home, mine);
        }

        let owned = of(&home, DeploymentKind::Hub, Some(&config));
        let cleared = remove(&home, &owned, false, &[]).expect("removes");

        assert_eq!(cleared, Cleared::default(), "a home is never removed");
        for mine in [
            "holiday-photos.txt",
            ".bashrc",
            "configs/my-editor.yaml",
            "secrets/my-own.key",
            "mounts/somebody-elses/main.py",
            "Documents/thesis.tex",
        ] {
            assert!(home.join(mine).is_file(), "{mine} is not Konstruktor's");
        }
        let mut left: Vec<String> = std::fs::read_dir(&home)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                ".bashrc",
                "Documents",
                "configs",
                "holiday-photos.txt",
                "mounts",
                "secrets"
            ]
        );
        assert_eq!(std::fs::read_dir(home.join("configs")).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(home.join("secrets")).unwrap().count(), 1);

        std::fs::remove_dir_all(&home).ok();
    }

    /// The ordinary case is unchanged in what it leaves: a folder that held only a hub
    /// is gone.
    #[test]
    fn a_folder_that_held_only_the_hub_goes_with_it() {
        let dir = scratch("only-hub");
        let config = a_hub(&dir);

        let owned = of(&dir, DeploymentKind::Hub, Some(&config));
        let cleared = remove(&dir, &owned, true, &[]).expect("removes");

        assert!(cleared.folder_removed, "left {:?}", cleared.left_behind);
        assert!(!dir.exists());
    }

    /// A folder that may go, and holds something else, stays and says what.
    #[test]
    fn a_folder_with_something_else_in_it_stays_and_names_it() {
        let dir = scratch("shared");
        let config = a_hub(&dir);
        put(&dir, "notes.md");

        let owned = of(&dir, DeploymentKind::Hub, Some(&config));
        let cleared = remove(&dir, &owned, true, &[]).expect("removes");

        assert!(!cleared.folder_removed);
        assert_eq!(cleared.left_behind, ["notes.md"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The lock is a file a person can edit, and a path in it is not a licence.
    #[test]
    fn a_recorded_path_outside_the_folder_is_not_followed() {
        let root = scratch("escape");
        let dir = root.join("hub");
        std::fs::create_dir_all(&dir).unwrap();
        put(&dir, HUB_CONFIG_FILENAME);
        put(&root, "precious.txt");
        let outside = root.join("precious.txt").display().to_string();
        std::fs::write(
            dir.join("hub_lock.json"),
            serde_json::json!({"version": 1, "files": {"../precious.txt": "00", outside: "00"}})
                .to_string(),
        )
        .unwrap();

        let owned = of(&dir, DeploymentKind::Hub, None);
        remove(&dir, &owned, true, &[]).expect("removes");

        assert!(root.join("precious.txt").is_file());
        assert!(!dir.exists());

        std::fs::remove_dir_all(&root).ok();
    }

    /// A linked `configs/` is somewhere else, and what is in it stays there.
    #[cfg(unix)]
    #[test]
    fn nothing_is_removed_through_a_link() {
        let root = scratch("link");
        let dir = root.join("hub");
        std::fs::create_dir_all(&dir).unwrap();
        put(&dir, HUB_CONFIG_FILENAME);
        put(&root, "elsewhere/deployer.yaml");
        put(&root, "data/rows");
        std::os::unix::fs::symlink(root.join("elsewhere"), dir.join("configs")).unwrap();
        std::os::unix::fs::symlink(root.join("data"), dir.join(".konstruktor")).unwrap();

        let owned = of(&dir, DeploymentKind::Engine, None);
        let cleared = remove(&dir, &owned, true, &[]).expect("removes");

        assert!(root.join("elsewhere/deployer.yaml").is_file());
        assert!(root.join("data/rows").is_file());
        assert_eq!(
            cleared.left_behind,
            [".konstruktor", "configs", HUB_CONFIG_FILENAME]
        );

        std::fs::remove_dir_all(&root).ok();
    }

    /// A hub of `example` alone, run from a checkout of the repository its image names.
    fn from_source() -> HubConfig {
        use crate::catalog::ServiceId;
        use crate::config::hub::ServiceOptions;

        let mut example = crate::support::example();
        example.source = Some(crate::contract::Source {
            repository: "https://git.example.org/me/example".into(),
            revision: None,
            path: "/srv/example".into(),
        });
        let mut said = crate::support::said();
        said.insert("example".to_string(), example);
        let id = ServiceId::named("example");
        let mut config = build_hub_config(&HubConfigOptions {
            services: Some(vec![id]),
            rekuest_server: "none".into(),
            service_options: std::collections::BTreeMap::from([(
                id,
                ServiceOptions {
                    from_source: true,
                    ..Default::default()
                },
            )]),
            ..Default::default()
        });
        config.set_service_image("example", "example:1");
        config.provide(&said);
        config
    }

    /// A checkout is a tree only when the lock says Konstruktor cloned it. One that was
    /// there before the hub — even for a service the hub runs from it — is somebody's
    /// own, like whatever else they keep under `mounts/`.
    #[test]
    fn only_the_checkout_of_a_service_goes_from_mounts() {
        let config = from_source();

        let dir = scratch("checkouts");
        put(&dir, HUB_CONFIG_FILENAME);
        put(&dir, "mounts/example/src/main.py");
        put(&dir, "mounts/my-own-project/main.py");

        let before = of(&dir, DeploymentKind::Hub, Some(&config));
        assert!(before.trees.is_empty(), "nothing says it was cloned");

        crate::lock::record_checkout(&dir, "example").unwrap();
        let owned = of(&dir, DeploymentKind::Hub, Some(&config));
        assert_eq!(owned.checkouts(&dir), ["example"]);
        let cleared = remove(&dir, &owned, true, &[]).expect("removes");

        assert!(!dir.join("mounts/example").exists());
        assert!(dir.join("mounts/my-own-project/main.py").is_file());
        assert_eq!(cleared.left_behind, ["mounts"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A tree linked in as a service's checkout is used where it is: the link goes with
    /// the hub, and the tree it points at is exactly as it was.
    #[cfg(unix)]
    #[test]
    fn a_linked_checkout_loses_its_link_and_keeps_its_tree() {
        let root = scratch("linked-checkout");
        let dir = root.join("hub");
        std::fs::create_dir_all(dir.join("mounts")).unwrap();
        put(&dir, HUB_CONFIG_FILENAME);
        put(&root, "my-tree/manage.py");
        std::os::unix::fs::symlink(root.join("my-tree"), dir.join("mounts/example")).unwrap();

        let owned = of(&dir, DeploymentKind::Hub, Some(&from_source()));
        assert!(owned.trees.is_empty());
        let cleared = remove(&dir, &owned, true, &[]).expect("removes");

        assert!(cleared.folder_removed, "left {:?}", cleared.left_behind);
        assert!(root.join("my-tree/manage.py").is_file());

        std::fs::remove_dir_all(&root).ok();
    }

    /// An engine is its compose file and its identity, and nothing of a hub's.
    #[test]
    fn an_engine_folder_goes_by_its_own_names() {
        let dir = scratch("engine");
        put(&dir, COMPOSE_FILENAME);
        put(&dir, crate::engine::CONFIG_FILE);
        put(&dir, crate::engine::MESH_FILE);

        let owned = of(&dir, DeploymentKind::Engine, None);
        let cleared = remove(&dir, &owned, true, &[]).expect("removes");

        assert!(cleared.folder_removed, "left {:?}", cleared.left_behind);
    }
}
