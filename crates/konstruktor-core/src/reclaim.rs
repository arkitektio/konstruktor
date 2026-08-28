//! Getting a hub's data directories back so they can be deleted.
//!
//! A hub keeps its database and its object storage in **bind mounts inside its own
//! folder** — `./db_data` and `./minio_data` by default. That is deliberate (see
//! `config::hub`), but it has a consequence nothing else in the app had to deal with: the
//! Docker daemon runs as root, creates those directories itself on the first `compose
//! up`, and the containers write into them as root. A desktop user then cannot delete
//! them, and `remove_dir_all` fails with `EACCES`.
//!
//! Two jobs live here, and they are deliberately separate:
//!
//! * **Deciding which directories are ours to touch.** [`resolve_mount`] is the guard. A
//!   mount is a string out of a YAML file that a person can edit, so it is treated as
//!   untrusted input: absolute paths, `..`, symlinks and the deployment's own files are
//!   refused rather than followed.
//! * **Getting past root ownership.** [`remove_tree`] removes a directory, and only if
//!   that fails on a permission error does it try to repair ownership and retry once.
//!
//! The repair borrows the daemon — the one thing on the machine that is already root —
//! to run `chown`, and nothing else. The removal itself stays on the host, so every guard
//! in `destroy` still governs what can be deleted. A container doing the removal would
//! have none of them, and a wrong mount source would be unbounded.
//!
//! **Not done here, on purpose:** pulling an image to perform the repair (a destructive
//! action the user is watching must not turn into a download), and elevating with
//! `sudo`/`pkexec` (a far larger trust ask than reusing a daemon the user has already
//! granted root to, and unavailable on macOS and Windows anyway).
//!
//! **Known limitation.** Under Docker's `userns-remap`, an in-container `chown` to uid
//! *N* lands on the host as `subuid_base + N`, so the repair does not actually change the
//! ownership and the retry fails. The user is told what was tried. Rootless Docker has no
//! such problem, because container root already *is* the host user.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::hub::HubConfig;
use crate::docker;
use crate::profile::HUB_CONFIG_FILENAME;

/// Files the deployment folder owns, which no mount may resolve onto however the profile
/// is written. Belt and braces: `resolve_mount` already refuses anything that is not a
/// strict descendant, and this refuses the descendants that are not data.
const RESERVED: &[&str] = &[
    "configs",
    "mounts",
    HUB_CONFIG_FILENAME,
    "docker-compose.yaml",
    "hub_credentials.json",
];

/// Why a mount in the profile is not something Konstruktor will delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum MountRefusal {
    #[error("it is an absolute path, outside the deployment folder")]
    Absolute,
    #[error("it points outside the deployment folder")]
    Escapes,
    #[error("it is the deployment folder itself")]
    TheFolderItself,
    #[error("it is one of the deployment's own files, not data")]
    Reserved,
    #[error("it is a symbolic link, which may lead anywhere")]
    Symlink,
}

/// Where a mount from a hub profile actually lands on this machine.
///
/// The checks are **string-level before they are filesystem-level**, and that ordering is
/// load-bearing: `Path::is_absolute` answers `false` for `C:\data` when the path is parsed
/// on Linux, and a profile written on Windows can be read on Linux. Anything that looks
/// absolute to *any* platform is refused here rather than joined onto the folder.
///
/// `Ok(None)` means there is nothing to do — the mount is empty, which in the generator's
/// vocabulary means "use a named volume", and a named volume is `docker compose down
/// --volumes`'s business rather than ours.
pub fn resolve_mount(dir: &Path, mount: &str) -> Result<Option<PathBuf>, MountRefusal> {
    let raw = mount.trim();
    if raw.is_empty() {
        return Ok(None);
    }

    let mut chars = raw.chars();
    let first = chars.next().unwrap_or('\0');
    // A unix absolute path, a UNC path, or `~` — none of which belong under the folder.
    if first == '/' || first == '\\' || first == '~' {
        return Err(MountRefusal::Absolute);
    }
    // `C:\data` and `c:/data`. Checked by shape, not by `Path`, for the reason above.
    if first.is_ascii_alphabetic() && chars.next() == Some(':') {
        return Err(MountRefusal::Absolute);
    }

    // Both separators, because the profile may have been written on either platform.
    let mut segments: Vec<&str> = Vec::new();
    for segment in raw.split(['/', '\\']) {
        match segment {
            "" | "." => continue,
            ".." => return Err(MountRefusal::Escapes),
            other => segments.push(other),
        }
    }

    let Some(first_segment) = segments.first() else {
        // `.` or `./` — the folder itself, which the caller must never delete as "data".
        return Err(MountRefusal::TheFolderItself);
    };
    if RESERVED.iter().any(|name| name == first_segment) {
        return Err(MountRefusal::Reserved);
    }

    let resolved = segments
        .iter()
        .fold(dir.to_path_buf(), |acc, s| acc.join(s));

    // A directory that is not there yet is not a refusal — the stack may never have run.
    // There is simply nothing to remove, and the caller treats it as done.
    let Ok(metadata) = std::fs::symlink_metadata(&resolved) else {
        return Ok(None);
    };
    if metadata.file_type().is_symlink() {
        return Err(MountRefusal::Symlink);
    }

    // The last guard, and the one that catches a symlinked *component* part-way along the
    // path: resolve both ends and require a strict descendant.
    let (Ok(canonical), Ok(root)) = (std::fs::canonicalize(&resolved), std::fs::canonicalize(dir))
    else {
        return Err(MountRefusal::Escapes);
    };
    if canonical == root || !canonical.starts_with(&root) {
        return Err(MountRefusal::Escapes);
    }

    Ok(Some(canonical))
}

/// A mount the purge left alone, and why.
#[derive(Debug, Clone, Serialize)]
pub struct SkippedMount {
    /// The mount exactly as the profile spells it.
    pub mount: String,
    pub refusal: MountRefusal,
    /// The refusal in words, so a front end does not have to know the enum.
    pub explanation: String,
}

/// The bind-mounted data directories of one hub, and the mounts that were refused.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DataDirs {
    pub removable: Vec<PathBuf>,
    pub skipped: Vec<SkippedMount>,
}

/// Which image to run the repair with, for a given data directory.
///
/// The image of the service that wrote the directory, first: it provably ran on this
/// machine, or the root-owned directory would not exist. The rest are fallbacks for the
/// whole-folder case, where no single service owns the tree.
pub fn repair_images(config: &HubConfig) -> Vec<String> {
    let mut images = vec![
        config.db.image.clone(),
        config.minio.image.clone(),
        config.gateway.image.clone(),
        config.local_redis.image.clone(),
    ];
    images.dedup();
    images
}

/// The data directories of a hub, resolved against its folder.
///
/// Nested and duplicate mounts are folded together: removing an inner directory after its
/// parent has gone would fail with `NotFound` and report a problem that is not one.
pub fn data_dirs(dir: &Path, config: &HubConfig) -> DataDirs {
    let mut found = DataDirs::default();

    for mount in [config.db.mount.as_deref(), config.minio.mount.as_deref()]
        .into_iter()
        .flatten()
    {
        match resolve_mount(dir, mount) {
            Ok(Some(path)) => found.removable.push(path),
            Ok(None) => {}
            Err(refusal) => found.skipped.push(SkippedMount {
                mount: mount.to_string(),
                explanation: refusal.to_string(),
                refusal,
            }),
        }
    }

    found.removable = fold_nested(found.removable);
    found
}

/// Sorts, dedupes, and drops any directory already contained by another.
///
/// Two mounts can name the same tree, or one inside the other. Removing the inner one
/// after its parent has gone would fail with `NotFound` and report a problem that is not
/// one. Sorting puts a parent before everything beneath it, so a single pass suffices.
fn fold_nested(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort();
    paths.dedup();

    let mut kept: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !kept.iter().any(|parent| path.starts_with(parent)) {
            kept.push(path);
        }
    }
    kept
}

#[derive(Debug, thiserror::Error)]
pub enum ReclaimError {
    #[error("{path}: {source}")]
    NotRemoved {
        path: String,
        source: std::io::Error,
    },
    #[error(
        "{path}: {first}. Docker created this as root; Konstruktor tried to hand it back \
         ({repair}) and still could not remove it: {second}"
    )]
    StillNotRemoved {
        path: String,
        first: std::io::Error,
        second: std::io::Error,
        repair: String,
    },
}

/// Whether an error is the "you are not root and Docker was" case.
///
/// Wider than `ErrorKind::PermissionDenied` alone: `remove_dir_all` surfaces the errno of
/// whichever `unlink` or `rmdir` failed, and those do not always map to that kind.
fn is_permission(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::PermissionDenied
        || matches!(error.raw_os_error(), Some(13) | Some(1))
}

/// Removes a directory tree, repairing ownership once if permission is refused.
///
/// `owner_of` is the folder whose uid and gid the tree should end up owned by — always
/// the deployment folder, never the tree itself, which is exactly the thing root took.
///
/// One retry, not a loop: a second permission error means the repair did not work, and
/// asking again will not change that.
pub fn remove_tree(path: &Path, owner_of: &Path, images: &[String]) -> Result<(), ReclaimError> {
    let shown = path.display().to_string();

    let first = match std::fs::remove_dir_all(path) {
        Ok(()) => return Ok(()),
        Err(error) if !is_permission(&error) => {
            return Err(ReclaimError::NotRemoved {
                path: shown,
                source: error,
            })
        }
        Err(error) => error,
    };

    let repair = match repair_ownership(path, owner_of, images) {
        Ok(note) => note,
        Err(note) => {
            return Err(ReclaimError::StillNotRemoved {
                path: shown,
                first,
                second: std::io::Error::new(std::io::ErrorKind::PermissionDenied, note.clone()),
                repair: note,
            })
        }
    };

    std::fs::remove_dir_all(path).map_err(|second| ReclaimError::StillNotRemoved {
        path: shown,
        first,
        second,
        repair,
    })
}

/// Hands a tree back to the owner of `owner_of`, using the daemon's root.
///
/// `Ok` carries what was done, `Err` why it could not be, and both end up in the message
/// the user reads — "we could not repair it" is only useful with the reason attached.
#[cfg(unix)]
fn repair_ownership(path: &Path, owner_of: &Path, images: &[String]) -> Result<String, String> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::metadata(owner_of)
        .map_err(|e| format!("could not read {}: {e}", owner_of.display()))?;
    let (uid, gid) = (metadata.uid(), metadata.gid());

    let image = images
        .iter()
        .find(|image| docker::image_present(image))
        .ok_or_else(|| {
            format!(
                "none of this hub's images are still on this machine ({}), so there was \
                 nothing to run `chown` with",
                images.join(", ")
            )
        })?;

    let args =
        docker::chown_args(&path.display().to_string(), image, uid, gid).ok_or_else(|| {
            format!(
                "the path {} contains a character that cannot be passed to `docker run`",
                path.display()
            )
        })?;

    let output = std::process::Command::new("docker")
        .args(&args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("could not run docker: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "`docker run … chown` failed: {}",
            stderr.trim().lines().next().unwrap_or("no output")
        ));
    }

    Ok(format!("using {image}"))
}

/// On Windows the problem is a different one and no container is involved.
///
/// Docker Desktop presents bind mounts as the calling user, so the root-owned case does
/// not arise. What does make `remove_dir_all` fail with a permission error there is the
/// read-only attribute, so that is what gets cleared.
#[cfg(windows)]
fn repair_ownership(path: &Path, _owner_of: &Path, _images: &[String]) -> Result<String, String> {
    fn clear(path: &Path) -> std::io::Result<()> {
        let metadata = std::fs::symlink_metadata(path)?;
        let mut permissions = metadata.permissions();
        if permissions.readonly() {
            permissions.set_readonly(false);
            std::fs::set_permissions(path, permissions)?;
        }
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            for entry in std::fs::read_dir(path)? {
                clear(&entry?.path())?;
            }
        }
        Ok(())
    }

    clear(path).map_err(|e| format!("could not clear read-only files: {e}"))?;
    Ok("clearing read-only files".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory of our own — there is no `tempfile` here, and these need real
    /// paths for `symlink_metadata` and `canonicalize` to have anything to say.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "konstruktor-reclaim-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    /// The guard, on strings alone. Everything here is refused before the filesystem is
    /// consulted at all, which is why a Windows-style path is in the list: a profile
    /// written on Windows can be read on Linux, where `Path::is_absolute` would say
    /// `false` for `C:\data` and happily join it onto the deployment folder.
    #[test]
    fn refuses_every_mount_that_is_not_plainly_inside_the_folder() {
        let dir = scratch("refusals");

        for (mount, expected) in [
            ("/etc", MountRefusal::Absolute),
            ("\\\\server\\share", MountRefusal::Absolute),
            ("C:\\data", MountRefusal::Absolute),
            ("c:/data", MountRefusal::Absolute),
            ("~/x", MountRefusal::Absolute),
            ("../evil", MountRefusal::Escapes),
            ("..\\evil", MountRefusal::Escapes),
            ("sub/../../x", MountRefusal::Escapes),
            (".", MountRefusal::TheFolderItself),
            ("./", MountRefusal::TheFolderItself),
            ("configs", MountRefusal::Reserved),
            ("mounts/rekuest", MountRefusal::Reserved),
            ("./hub_config.yaml", MountRefusal::Reserved),
        ] {
            assert_eq!(
                resolve_mount(&dir, mount),
                Err(expected),
                "{mount} should have been refused"
            );
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    /// An empty mount is the generator's way of saying "use a named volume", which
    /// `down --volumes` already removes. Nothing to do is not the same as a refusal.
    #[test]
    fn an_empty_mount_is_a_named_volume_and_not_our_business() {
        let dir = scratch("named");
        assert_eq!(resolve_mount(&dir, ""), Ok(None));
        assert_eq!(resolve_mount(&dir, "   "), Ok(None));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn accepts_a_directory_inside_the_folder() {
        let dir = scratch("accepts");
        std::fs::create_dir_all(dir.join("db_data")).unwrap();
        std::fs::create_dir_all(dir.join("data").join("minio")).unwrap();

        let root = std::fs::canonicalize(&dir).unwrap();
        assert_eq!(
            resolve_mount(&dir, "./db_data"),
            Ok(Some(root.join("db_data")))
        );
        assert_eq!(
            resolve_mount(&dir, "data/minio"),
            Ok(Some(root.join("data").join("minio")))
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A directory the stack never created is nothing to remove, not a failure.
    #[test]
    fn a_missing_directory_is_simply_nothing_to_do() {
        let dir = scratch("missing");
        assert_eq!(resolve_mount(&dir, "./db_data"), Ok(None));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The one that matters most: a symlink is how you would get past every string check
    /// above, so it is refused outright and whatever it pointed at survives.
    #[cfg(unix)]
    #[test]
    fn refuses_a_symlinked_data_directory_and_leaves_its_target_alone() {
        let dir = scratch("symlink");
        let elsewhere = scratch("symlink-target");
        std::fs::write(elsewhere.join("precious.txt"), "do not delete").unwrap();
        std::os::unix::fs::symlink(&elsewhere, dir.join("db_data")).unwrap();

        assert_eq!(resolve_mount(&dir, "./db_data"), Err(MountRefusal::Symlink));
        assert!(
            elsewhere.join("precious.txt").exists(),
            "the symlink's target must be untouched"
        );

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&elsewhere).ok();
    }

    /// A component part-way along the path can be a link too, which no amount of string
    /// checking catches — this is what canonicalizing both ends is for.
    #[cfg(unix)]
    #[test]
    fn refuses_a_path_that_leaves_through_a_symlinked_component() {
        let dir = scratch("component");
        let elsewhere = scratch("component-target");
        std::fs::create_dir_all(elsewhere.join("db")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, dir.join("data")).unwrap();

        assert_eq!(resolve_mount(&dir, "data/db"), Err(MountRefusal::Escapes));
        assert!(elsewhere.join("db").exists());

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&elsewhere).ok();
    }

    #[test]
    fn folds_a_nested_or_repeated_mount_into_one_entry() {
        let p = |s: &str| PathBuf::from(s);

        assert_eq!(
            fold_nested(vec![p("/hub/data/minio"), p("/hub/data")]),
            vec![p("/hub/data")],
            "an inner directory goes with its parent"
        );
        assert_eq!(
            fold_nested(vec![p("/hub/db_data"), p("/hub/db_data")]),
            vec![p("/hub/db_data")],
            "the same mount twice is one removal"
        );
        assert_eq!(
            fold_nested(vec![p("/hub/minio_data"), p("/hub/db_data")]),
            vec![p("/hub/db_data"), p("/hub/minio_data")],
            "siblings both survive"
        );
        // Prefix-of-a-name is not containment: `db_data2` is not inside `db_data`.
        assert_eq!(
            fold_nested(vec![p("/hub/db_data"), p("/hub/db_data2")]),
            vec![p("/hub/db_data"), p("/hub/db_data2")]
        );
    }

    /// `data_dirs` over a real config, so the wiring from profile fields to resolved
    /// directories is covered rather than assumed.
    #[test]
    fn reads_both_data_directories_off_a_real_config() {
        use crate::config::hub::{build_hub_config, HubConfigOptions};

        let dir = scratch("config");
        std::fs::create_dir_all(dir.join("db_data")).unwrap();
        std::fs::create_dir_all(dir.join("minio_data")).unwrap();
        let root = std::fs::canonicalize(&dir).unwrap();

        let mut config = build_hub_config(&HubConfigOptions::default());
        let found = data_dirs(&dir, &config);
        assert_eq!(
            found.removable,
            vec![root.join("db_data"), root.join("minio_data")]
        );
        assert!(found.skipped.is_empty());

        // The fixture `hub_config.yaml` really does carry an absolute minio mount, so this
        // is the shape of a hub in the wild, not a hypothetical.
        config.minio.mount = Some("/data".into());
        let found = data_dirs(&dir, &config);
        assert_eq!(found.removable, vec![root.join("db_data")]);
        assert_eq!(found.skipped.len(), 1);
        assert_eq!(found.skipped[0].refusal, MountRefusal::Absolute);
        assert_eq!(found.skipped[0].mount, "/data");

        // A named volume contributes nothing and is not a refusal.
        config.minio.mount = None;
        let found = data_dirs(&dir, &config);
        assert_eq!(found.removable, vec![root.join("db_data")]);
        assert!(found.skipped.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The fast path must not reach for Docker at all. An empty image list means a repair
    /// could only fail, so a green test proves no repair was attempted.
    #[test]
    fn removes_an_ordinary_tree_without_any_repair() {
        let dir = scratch("ordinary");
        let target = dir.join("db_data");
        std::fs::create_dir_all(target.join("deep")).unwrap();
        std::fs::write(target.join("deep").join("file.txt"), "x").unwrap();

        remove_tree(&target, &dir, &[]).expect("a user-owned tree needs no repair");
        assert!(!target.exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn maps_the_permission_errnos_and_nothing_else() {
        assert!(is_permission(&std::io::Error::from_raw_os_error(13))); // EACCES
        assert!(is_permission(&std::io::Error::from_raw_os_error(1))); // EPERM
        assert!(!is_permission(&std::io::Error::from_raw_os_error(2))); // ENOENT
    }

    /// Pins the argument order, which is the usual `docker run` trap: the image reference
    /// comes before the entrypoint's own arguments, not after them.
    #[test]
    fn builds_the_exact_chown_invocation() {
        let args = docker::chown_args("/home/someone/MyHub/db_data", "img:tag", 1000, 1000)
            .expect("an ordinary path");
        assert_eq!(
            args,
            vec![
                "run",
                "--rm",
                "--network",
                "none",
                "--read-only",
                "--pull=never",
                "--user",
                "0:0",
                "--entrypoint",
                "chown",
                "--mount",
                "type=bind,source=/home/someone/MyHub/db_data,target=/target",
                "img:tag",
                "-Rh",
                "1000:1000",
                "/target",
            ]
        );
    }

    /// `--mount` has no quoting, so a comma in the path would start another option. There
    /// is no portable escape for it; refusing and saying so beats guessing.
    #[test]
    fn refuses_a_path_that_cannot_be_passed_to_docker() {
        assert!(docker::chown_args("/home/a,b/db_data", "img", 1000, 1000).is_none());
        assert!(docker::chown_args("/home/a\nb/db_data", "img", 1000, 1000).is_none());
    }
}
