//! The hub folders earlier releases generated, and what this build makes of them: nothing.
//!
//! `fixtures/releases/<version>/` is what that release wrote for a default hub
//! (`scripts/capture-release-fixture.sh`). A hub out there has exactly such files. Their
//! profile is of the shape from before services were data — a key per service, and none
//! of what each service's image has since been asked — and this build does not migrate
//! it: such a hub is refused, told to be created again, and left exactly as it was found.

use std::path::{Path, PathBuf};

use konstruktor_core::profile::{self, ProfileError};

/// Each captured release.
const RELEASES: [&str; 3] = ["0.11.0", "0.13.0", "0.14.0"];

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn hub_of(version: &str, tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "konstruktor-release-{version}-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    copy(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/releases")
            .join(version),
        &dir,
    );
    dir
}

/// Every file under `dir`, by path, so "nothing was touched" can be an equality.
fn snapshot(dir: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).unwrap().flatten() {
            if entry.path().is_dir() {
                pending.push(entry.path());
            } else {
                out.insert(entry.path(), std::fs::read(entry.path()).unwrap());
            }
        }
    }
    out
}

#[test]
fn a_hub_of_an_earlier_release_is_refused_and_told_to_be_created_again() {
    for version in RELEASES {
        let dir = hub_of(version, "read");
        let refused = profile::read_profile(&dir)
            .err()
            .unwrap_or_else(|| panic!("{version}'s profile must not be read"));
        assert!(
            matches!(refused, ProfileError::Earlier { .. }),
            "{version}: {refused}"
        );
        let said = refused.to_string();
        assert!(said.contains("earlier konstruktor"), "{version}: {said}");
        assert!(said.contains("created again"), "{version}: {said}");
        assert!(said.contains("konstruktor hub create"), "{version}: {said}");
        assert!(
            said.contains(&*dir.to_string_lossy()),
            "{version}: it names the folder: {said}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

/// Nothing that rewrites a hub's files gets as far as writing one: regenerating, moving
/// an image and updating all read the profile first, and stop there.
#[tokio::test]
async fn nothing_rewrites_a_hub_of_an_earlier_release() {
    use konstruktor_core::updates::{self, UpdateRequest};

    for version in RELEASES {
        let dir = hub_of(version, "rewrite");
        let before = snapshot(&dir);

        assert!(matches!(
            profile::regenerate(&dir),
            Err(ProfileError::Earlier { .. })
        ));
        assert!(matches!(
            profile::rewrite_images(&dir, &[("mikro".into(), "jhnnsrs/mikro:7".into())]),
            Err(ProfileError::Earlier { .. })
        ));
        let refused = updates::apply(
            &dir,
            &UpdateRequest {
                services: vec!["rekuest".into()],
                advances: Vec::new(),
                pull: true,
                backup_into: Some(dir.join("backups")),
                health_check: true,
            },
            &|_| {},
        )
        .await
        .unwrap_err();
        assert!(
            refused.to_string().contains("created again"),
            "{version}: {refused}"
        );

        assert_eq!(snapshot(&dir), before, "{version}: the folder is as it was");
        std::fs::remove_dir_all(&dir).ok();
    }
}
