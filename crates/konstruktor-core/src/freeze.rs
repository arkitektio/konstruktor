//! Telling an update to leave a hub, or some of its services, alone.
//!
//! A hub already runs exact builds: its compose file names them, and only `update` looks
//! for newer ones ([`crate::pins`]). Freezing is the other half of that — a mark, in
//! `hub_lock.json`, that says `update` is not to look for these. A frozen service is
//! skipped and said; and an update that would have to move the hub's files to another
//! layout, which takes every service along, is refused while anything is frozen.
//!
//! It is a mark and nothing else: no file is rewritten, nothing is fetched, nothing
//! restarts. Lifting it changes nothing either, until the next `update`.

use std::collections::BTreeMap;
use std::path::Path;

use crate::config::hub::HubConfig;
use crate::lock::{self, Frozen};
use crate::profile::read_profile;

#[derive(Debug, thiserror::Error)]
pub enum FreezeError {
    #[error("{0}")]
    Profile(String),
    #[error("this hub runs no service called `{0}`")]
    NoSuchService(String),
    #[error("nothing here is frozen")]
    NothingFrozen,
    #[error("{0}")]
    Write(#[from] std::io::Error),
}

/// The services a request names, each with what moves with it (takt, for Rekuest) — or
/// every image the stack declares when it names none.
fn named(config: &HubConfig, services: &[String]) -> Result<Vec<String>, FreezeError> {
    let stack: Vec<String> = config
        .stack_images()
        .into_iter()
        .map(|(service, _)| service)
        .collect();
    if services.is_empty() {
        return Ok(stack);
    }
    let mut names: Vec<String> = Vec::new();
    for service in services {
        if !stack.contains(service) {
            return Err(FreezeError::NoSuchService(service.clone()));
        }
        for name in std::iter::once(service.clone())
            .chain(crate::generate::compose::companions(config, service))
        {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    Ok(names)
}

fn config_of(dir: &Path) -> Result<HubConfig, FreezeError> {
    read_profile(dir)
        .map(|profile| profile.config)
        .map_err(|e| FreezeError::Profile(e.to_string()))
}

/// Marks `services` (everything the hub runs, when empty) as left alone by `update`.
/// Returns the ones newly frozen; one that already was keeps the day it was frozen.
pub fn hold(dir: &Path, services: &[String]) -> Result<Vec<String>, FreezeError> {
    let names = named(&config_of(dir)?, services)?;
    let mut held = lock::read(dir);
    held.version = 1;
    let at = lock::now();
    let newly: Vec<String> = names
        .into_iter()
        .filter(|name| !held.frozen.contains_key(name))
        .collect();
    for name in &newly {
        held.frozen.insert(name.clone(), Frozen { at });
    }
    if !newly.is_empty() {
        lock::write(dir, &held)?;
    }
    Ok(newly)
}

/// Lifts the mark from `services` (everything frozen, when empty). Returns what was
/// released.
pub fn release(dir: &Path, services: &[String]) -> Result<Vec<String>, FreezeError> {
    let mut held = lock::read(dir);
    let names: Vec<String> = match services.is_empty() {
        true => held.frozen.keys().cloned().collect(),
        false => named(&config_of(dir)?, services)?
            .into_iter()
            .filter(|name| held.frozen.contains_key(name))
            .collect(),
    };
    if names.is_empty() {
        return Err(FreezeError::NothingFrozen);
    }
    held.frozen.retain(|service, _| !names.contains(service));
    lock::write(dir, &held)?;
    Ok(names)
}

/// What is frozen in the hub in `dir`, by compose service.
pub fn frozen(dir: &Path) -> BTreeMap<String, Frozen> {
    lock::read(dir).frozen
}

/// Why `service` is not moved, if it is frozen.
pub fn reason(frozen: &BTreeMap<String, Frozen>, service: &str) -> Option<String> {
    frozen.get(service).map(|held| {
        format!(
            "`{service}` is frozen since {} and was left on the build it runs. \
             `konstruktor unfreeze` lets `update` move it again.",
            crate::backup::timestamp(held.at)
        )
    })
}

/// Marks exactly `services` — no companions added, no check against the profile — as left
/// alone: for a rollback, which names every service it put back itself.
pub fn hold_exactly(dir: &Path, services: &[String]) -> std::io::Result<()> {
    let mut held = lock::read(dir);
    let at = lock::now();
    for service in services {
        held.frozen.entry(service.clone()).or_insert(Frozen { at });
    }
    lock::write(dir, &held)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::hub::{build_hub_config, HubConfigOptions};

    fn hub(tag: &str) -> (std::path::PathBuf, HubConfig) {
        let dir = std::env::temp_dir().join(format!(
            "konstruktor-freeze-{tag}-{}-{}",
            std::process::id(),
            lock::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config = build_hub_config(&HubConfigOptions::default());
        crate::profile::rewrite(&dir, config.clone(), &[]).unwrap();
        (dir, config)
    }

    /// A mark and nothing else: the files are what they were.
    #[test]
    fn freezing_and_releasing_write_no_file_but_the_lock() {
        let (dir, config) = hub("mark");
        let compose = std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap();
        let profile = std::fs::read_to_string(crate::profile::profile_path(&dir)).unwrap();

        let newly = hold(&dir, &[]).unwrap();
        assert_eq!(newly.len(), config.stack_images().len());
        assert_eq!(hold(&dir, &[]).unwrap(), Vec::<String>::new());
        assert!(reason(&frozen(&dir), "rekuest")
            .unwrap()
            .contains("konstruktor unfreeze"));
        assert_eq!(reason(&frozen(&dir), "nope"), None);

        assert_eq!(release(&dir, &[]).unwrap().len(), newly.len());
        assert!(frozen(&dir).is_empty());
        assert!(matches!(
            release(&dir, &[]),
            Err(FreezeError::NothingFrozen)
        ));
        assert_eq!(
            std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap(),
            compose
        );
        assert_eq!(
            std::fs::read_to_string(crate::profile::profile_path(&dir)).unwrap(),
            profile
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rekuest_takes_takt_along_and_a_service_the_hub_does_not_run_is_refused() {
        let (dir, _) = hub("named");
        assert_eq!(
            hold(&dir, &["rekuest".to_string()]).unwrap(),
            ["rekuest", "rekuest-takt"]
        );
        assert!(matches!(
            hold(&dir, &["nope".to_string()]),
            Err(FreezeError::NoSuchService(_))
        ));
        // Released by the same name, the pair goes together; the rest was never held.
        assert!(matches!(
            release(&dir, &["mikro".to_string()]),
            Err(FreezeError::NothingFrozen)
        ));
        assert_eq!(
            release(&dir, &["rekuest".to_string()]).unwrap(),
            ["rekuest", "rekuest-takt"]
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
