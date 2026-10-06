//! The hub folders earlier releases generated, and what this build makes of them.
//!
//! `fixtures/releases/<version>/` is what that release wrote for a default hub
//! (`scripts/capture-release-fixture.sh`). A hub out there has exactly such files, so
//! these are what "an existing hub" means to this build: which layout it reads them as,
//! and that rewriting them lands where a hub created today starts.

use std::path::{Path, PathBuf};

use konstruktor_core::generate::{generate_hub_files, IssuedIdentity};
use konstruktor_core::migrate;
use konstruktor_core::profile;

/// Each captured release, and the layout its files have.
const RELEASES: [(&str, u32); 3] = [("0.11.0", 1), ("0.13.0", 2), ("0.14.0", 3)];

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

#[test]
fn a_hub_of_an_earlier_release_is_read_as_the_layout_it_has() {
    for (version, layout) in RELEASES {
        let dir = hub_of(version, "read");
        let config = profile::read_profile(&dir)
            .unwrap_or_else(|error| panic!("{version}'s profile is read: {error}"))
            .config;
        assert_eq!(migrate::layout(&dir, &config), layout, "{version}");
        assert_eq!(
            migrate::pending(&dir, &config).len() as u32,
            migrate::CURRENT_LAYOUT - layout,
            "{version}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

/// What an update does to the files of such a hub: every generated file is what this build
/// generates from the hub's own profile, nothing of the old layout is left beside them,
/// and the hub is no longer behind.
#[test]
fn rewriting_a_hub_of_an_earlier_release_lands_on_what_is_generated_today() {
    for (version, _) in RELEASES {
        let dir = hub_of(version, "rewrite");
        let config = profile::read_profile(&dir).unwrap().config;
        profile::rewrite(&dir, config, &[]).unwrap();

        let config = profile::read_profile(&dir).unwrap().config;
        let expected = generate_hub_files(&config, &IssuedIdentity::default(), &Default::default());
        for (path, contents) in &expected {
            assert_eq!(
                &std::fs::read_to_string(dir.join(path)).unwrap(),
                contents,
                "{version}: {path}"
            );
        }
        assert_eq!(migrate::pending(&dir, &config), [], "{version}");
        assert_eq!(
            migrate::hand_edited(&dir),
            Vec::<String>::new(),
            "{version}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

/// A frozen hub whose files are behind is not moved in passing: the move would take every
/// service along, which is what the freeze forbids. It is refused before anything is
/// backed up, copied or written.
#[tokio::test]
async fn a_frozen_hub_of_an_earlier_release_is_not_updated() {
    use konstruktor_core::lock::{self, Frozen};
    use konstruktor_core::updates::{self, UpdateError, UpdateRequest};

    let dir = hub_of("0.14.0", "frozen");
    let mut held = lock::read(&dir);
    held.frozen.insert("mikro".into(), Frozen { at: 1 });
    lock::write(&dir, &held).unwrap();
    let compose = std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap();

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
        matches!(&refused, UpdateError::Frozen(why) if why.contains("konstruktor unfreeze")),
        "{refused}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap(),
        compose
    );
    assert!(!dir.join("backups").exists() && !dir.join(".konstruktor").exists());
    std::fs::remove_dir_all(&dir).ok();
}
