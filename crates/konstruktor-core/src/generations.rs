//! Copies of a hub's generated files, taken before they are rewritten.
//!
//! The lock records which images a hub ran; this keeps what they ran *on*. The files are
//! generated, so for a long time the way back was "generate them again" — which only holds
//! while the generator still writes what the older images read. Once it does not, putting
//! an image back needs the files of its time, and a rewrite that fails half way needs
//! somewhere to return to.
//!
//! A generation is a folder under `.konstruktor/generations/` holding the profile, the
//! compose file, `configs/` and `secrets/` as they were, and what the lock said about them
//! (layout, hashes). It is named by the second it was taken at. The last few are kept.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::lock;

pub const GENERATIONS_DIR: &str = ".konstruktor/generations";

/// How many are kept: more than any rollback reaches back through.
const KEPT: usize = 5;

/// The generated files outside the two folders.
const FILES: [&str; 2] = [
    crate::profile::HUB_CONFIG_FILENAME,
    crate::compose_file::COMPOSE_FILENAME,
];
const FOLDERS: [&str; 2] = ["configs", "secrets"];
const RECORD: &str = "generation.json";

/// What the lock said about the files a generation holds.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Record {
    layout: Option<u32>,
    generated_by: Option<String>,
    #[serde(default)]
    files: BTreeMap<String, String>,
}

pub fn path(dir: &Path, name: &str) -> PathBuf {
    dir.join(GENERATIONS_DIR).join(name)
}

fn copy_folder(from: &Path, to: &Path) -> std::io::Result<()> {
    if !from.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if entry.path().is_file() {
            // `copy` carries the permissions over, so a key file stays its owner's alone.
            std::fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// Copies the hub's generated files as they are now. Returns the generation's name.
pub fn take(dir: &Path, at: u64) -> std::io::Result<String> {
    let name = at.to_string();
    let target = path(dir, &name);
    std::fs::create_dir_all(&target)?;
    for file in FILES {
        if dir.join(file).is_file() {
            std::fs::copy(dir.join(file), target.join(file))?;
        }
    }
    for folder in FOLDERS {
        copy_folder(&dir.join(folder), &target.join(folder))?;
    }
    let held = lock::read(dir);
    let record = Record {
        layout: held.layout,
        generated_by: held.generated_by,
        files: held.files,
    };
    std::fs::write(
        target.join(RECORD),
        serde_json::to_string_pretty(&record).expect("a record always serializes"),
    )?;
    prune(dir);
    Ok(name)
}

/// Puts a generation's files back, and what the lock said about them. The files of a
/// folder that the generation does not hold are removed: they were written since.
pub fn restore(dir: &Path, name: &str) -> std::io::Result<()> {
    let source = path(dir, name);
    if !source.join(RECORD).is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("there is no copy of this hub's files named {name}"),
        ));
    }
    for file in FILES {
        if source.join(file).is_file() {
            std::fs::copy(source.join(file), dir.join(file))?;
        }
    }
    for folder in FOLDERS {
        if let Ok(entries) = std::fs::read_dir(dir.join(folder)) {
            for entry in entries.flatten() {
                if entry.path().is_file() && !source.join(folder).join(entry.file_name()).exists() {
                    std::fs::remove_file(entry.path())?;
                }
            }
        }
        copy_folder(&source.join(folder), &dir.join(folder))?;
    }
    let record: Record = std::fs::read_to_string(source.join(RECORD))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let mut held = lock::read(dir);
    held.layout = record.layout;
    held.generated_by = record.generated_by;
    held.files = record.files;
    lock::write(dir, &held)
}

/// The layout of the files a generation holds, or `None` when there is no such generation.
pub fn layout(dir: &Path, name: &str, config: &crate::config::hub::HubConfig) -> Option<u32> {
    let source = path(dir, name);
    let record: Record =
        serde_json::from_str(&std::fs::read_to_string(source.join(RECORD)).ok()?).ok()?;
    Some(crate::migrate::layout_of(&source, record.layout, config))
}

/// Every generation of the hub, oldest first.
pub fn list(dir: &Path) -> Vec<String> {
    let mut names: Vec<(u64, String)> = std::fs::read_dir(dir.join(GENERATIONS_DIR))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            Some((name.parse().ok()?, name))
        })
        .collect();
    names.sort();
    names.into_iter().map(|(_, name)| name).collect()
}

fn prune(dir: &Path) {
    let names = list(dir);
    let excess = names.len().saturating_sub(KEPT);
    for name in &names[..excess] {
        let _ = std::fs::remove_dir_all(path(dir, name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "konstruktor-generations-{tag}-{}-{}",
            std::process::id(),
            lock::now()
        ));
        std::fs::create_dir_all(dir.join("configs")).unwrap();
        dir
    }

    #[test]
    fn a_generation_puts_back_what_was_there_and_removes_what_came_since() {
        let dir = scratch("restore");
        std::fs::write(dir.join("docker-compose.yaml"), "services: {}\n").unwrap();
        std::fs::write(dir.join("configs/rekuest.yaml"), "old\n").unwrap();
        lock::stamp(
            &dir,
            2,
            &BTreeMap::from([("configs/rekuest.yaml".to_string(), "old\n".to_string())]),
        )
        .unwrap();

        let name = take(&dir, 100).unwrap();

        std::fs::write(dir.join("docker-compose.yaml"), "services: {a: {}}\n").unwrap();
        std::fs::write(dir.join("configs/rekuest.yaml"), "new\n").unwrap();
        std::fs::write(dir.join("configs/takt.yaml"), "new\n").unwrap();
        lock::stamp(&dir, 3, &BTreeMap::new()).unwrap();

        restore(&dir, &name).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap(),
            "services: {}\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("configs/rekuest.yaml")).unwrap(),
            "old\n"
        );
        assert!(!dir.join("configs/takt.yaml").exists());
        let held = lock::read(&dir);
        assert_eq!(held.layout, Some(2));
        assert!(held.files.contains_key("configs/rekuest.yaml"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn only_the_last_few_generations_are_kept() {
        let dir = scratch("prune");
        for at in 1..=8 {
            take(&dir, at).unwrap();
        }
        assert_eq!(list(&dir), ["4", "5", "6", "7", "8"]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
