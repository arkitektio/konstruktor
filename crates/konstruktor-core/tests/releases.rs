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
        let expected = generate_hub_files(&config, &IssuedIdentity::default());
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
