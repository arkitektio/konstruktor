//! Which layout a hub's generated files have, and what takes them to the current one.
//!
//! A hub keeps running the files it was last given. What the generator writes changes —
//! a service it gained (takt), a key a service renamed — and the images a hub follows
//! change with it, so files and images have to move together. For that a hub has to be
//! able to say which files it has: the layout number in `hub_lock.json`, bumped whenever
//! files of the older number would not run the images of the newer.
//!
//! A move is a regeneration: the profile holds everything the files are made from. What a
//! [`Step`] adds is its name, said before it happens. `konstruktor update` is the one path
//! across — it regenerates and moves every service, since files of a new layout and
//! images of an old one are exactly the hub that comes up healthy and does nothing. A
//! container the new files no longer name (Rekuest's reaper) goes with the `up` that
//! follows. Everything else that rewrites a hub's files asks [`behind`] first and refuses.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::compose_file::COMPOSE_FILENAME;
use crate::config::hub::HubConfig;
use crate::generate::GeneratedFiles;
use crate::lock;

/// The layout this build generates.
///
/// 1. Rekuest with a reaper, serving its agents itself.
/// 2. takt beside Rekuest, on the port agents reach.
/// 3. Rekuest and takt share a socket; Rekuest reads `services` and `hook_agents`.
/// 4. Services follow the major they were generated for, not `latest`: Rekuest 6, Mikro 5,
///    Kabinet 4, Elektro 3, Alpaka 3, Fluss 2, Lovekit 2, Kraph 1.
pub const CURRENT_LAYOUT: u32 = 4;

/// The newest layout written before layouts were recorded: what a hub with no record is
/// taken for unless its files say otherwise.
const LAST_UNRECORDED: u32 = 3;

/// One move from a layout to the next, as a front end shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub to: u32,
    pub title: String,
}

fn title(to: u32) -> &'static str {
    match to {
        2 => "takt runs beside Rekuest, in place of its reaper",
        3 => {
            "Rekuest and takt share a socket, and Rekuest knows its services and hook agents apart"
        }
        4 => "every service follows the major release these files are written for, not `latest`",
        _ => "the files this Konstruktor generates",
    }
}

/// The layout of the files in `dir`: what the lock recorded, or — for a hub written before
/// it recorded any — what the compose file on disk shows.
pub fn layout(dir: &Path, config: &HubConfig) -> u32 {
    layout_of(dir, lock::read(dir).layout, config)
}

/// [`layout`], of files in any folder — a kept copy of a hub's files, say — given what
/// was recorded about them.
pub fn layout_of(files: &Path, recorded: Option<u32>, config: &HubConfig) -> u32 {
    recorded.unwrap_or_else(|| unrecorded(files, config))
}

fn unrecorded(dir: &Path, config: &HubConfig) -> u32 {
    // Nothing generated yet is nothing to be behind with.
    let Ok(text) = std::fs::read_to_string(dir.join(COMPOSE_FILENAME)) else {
        return CURRENT_LAYOUT;
    };
    let Some(takt) = config.takt_host() else {
        return LAST_UNRECORDED;
    };
    let Ok(compose) = serde_norway::from_str::<serde_norway::Value>(&text) else {
        return LAST_UNRECORDED;
    };
    let service = |name: &str| compose.get("services").and_then(|s| s.get(name));
    if service(&config.rekuest.host).is_none() {
        return LAST_UNRECORDED;
    }
    match service(&takt) {
        None => 1,
        Some(takt)
            if takt
                .get("environment")
                .and_then(|env| env.get("TAKT_INTERNAL_BIND"))
                .is_none() =>
        {
            2
        }
        Some(_) => LAST_UNRECORDED,
    }
}

/// The moves between this hub's files and what this build generates, in order.
pub fn pending(dir: &Path, config: &HubConfig) -> Vec<Step> {
    (layout(dir, config) + 1..=CURRENT_LAYOUT)
        .map(|to| Step {
            to,
            title: title(to).to_string(),
        })
        .collect()
}

/// Why this hub's files must not be rewritten in passing, if they must not: they are of an
/// older layout, and only an update moves the images along with them.
pub fn behind(dir: &Path, config: &HubConfig) -> Option<String> {
    let steps = pending(dir, config);
    if steps.is_empty() {
        return None;
    }
    Some(format!(
        "this hub's files are from an earlier Konstruktor ({}), and what is generated now \
         only works with the releases it was written for. Run `konstruktor update` first: \
         it rewrites the files and moves the services together, and keeps a copy of the \
         files as they are. Nothing was changed.",
        steps
            .iter()
            .map(|step| step.title.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    ))
}

/// The images a move brings along: every service still on what an earlier Konstruktor
/// seeded, to the major this one generates for. As `(compose service, image)`.
///
/// The ones switched off too: a service added later starts from the image its block names.
pub fn caught_up_images(config: &HubConfig) -> Vec<(String, String)> {
    crate::catalog::HUB_SERVICE_ORDER
        .into_iter()
        .filter_map(|id| {
            let block = config.service(id);
            let image = crate::config::hub::caught_up_image(id, block.image.as_deref()?)?;
            Some((block.host.clone(), image))
        })
        .collect()
}

/// The services a move cannot bring along, with the image each runs: somebody chose it, and
/// whether it reads the files generated now is theirs to know.
pub fn unsupported_images(config: &HubConfig) -> Vec<(String, String)> {
    config
        .enabled_services()
        .into_iter()
        .filter_map(|id| {
            let block = config.service(id);
            let image = block.image.clone()?;
            (!crate::config::hub::is_supported_image(id, &image)
                && crate::config::hub::caught_up_image(id, &image).is_none())
            .then(|| (block.host.clone(), image))
        })
        .collect()
}

/// The generated files that differ from what was last written: edited by hand since.
/// Empty for a hub whose files were never recorded.
pub fn hand_edited(dir: &Path) -> Vec<String> {
    lock::read(dir)
        .files
        .into_iter()
        .filter(|(path, digest)| {
            std::fs::read(dir.join(path)).is_ok_and(|bytes| &lock::digest(&bytes) != digest)
        })
        .map(|(path, _)| path)
        .collect()
}

/// Writes a hub's generated files and records them: the layout they have, and each file's
/// hash. A file the record held and the generator no longer writes is removed.
pub fn write_hub(dir: &Path, files: &GeneratedFiles) -> std::io::Result<()> {
    crate::generate::write::write_generated_files(dir, files)?;
    let written: BTreeMap<String, String> = files.clone();
    for stale in lock::stamp(dir, CURRENT_LAYOUT, &written)? {
        let _ = std::fs::remove_file(dir.join(stale));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::hub::{build_hub_config, HubConfigOptions};

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "konstruktor-migrate-{tag}-{}-{}",
            std::process::id(),
            lock::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn config() -> HubConfig {
        build_hub_config(&HubConfigOptions::default())
    }

    /// The compose files hubs from before the record run, and the layout each is read as.
    #[test]
    fn a_hub_with_no_record_is_read_off_its_compose_file() {
        let config = config();
        let dir = scratch("unrecorded");
        let write = |text: &str| std::fs::write(dir.join(COMPOSE_FILENAME), text).unwrap();

        write("services:\n  rekuest: {}\n  rekuest-reaper: {}\n");
        assert_eq!(layout(&dir, &config), 1);
        assert_eq!(pending(&dir, &config).len(), 3);
        assert!(behind(&dir, &config)
            .unwrap()
            .contains("konstruktor update"));

        write("services:\n  rekuest: {}\n  rekuest-takt: {}\n");
        assert_eq!(layout(&dir, &config), 2);

        write(
            "services:\n  rekuest: {}\n  rekuest-takt:\n    environment:\n      TAKT_INTERNAL_BIND: unix:/run/takt/internal.sock\n",
        );
        assert_eq!(layout(&dir, &config), 3);
        assert_eq!(pending(&dir, &config).len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Once written by this build, the record answers — whatever the files look like.
    #[test]
    fn written_files_are_recorded_and_a_file_no_longer_generated_goes() {
        let config = config();
        let dir = scratch("write");
        let files = crate::generate::generate_hub_files(&config, &Default::default());
        let mut with_extra = files.clone();
        with_extra.insert("configs/gone.yaml".into(), "x: 1\n".into());

        write_hub(&dir, &with_extra).unwrap();
        assert!(dir.join("configs/gone.yaml").exists());
        write_hub(&dir, &files).unwrap();
        assert!(!dir.join("configs/gone.yaml").exists());

        std::fs::write(dir.join(COMPOSE_FILENAME), "services: {}\n").unwrap();
        assert_eq!(layout(&dir, &config), CURRENT_LAYOUT);
        assert_eq!(hand_edited(&dir), [COMPOSE_FILENAME]);
        assert_eq!(
            lock::read(&dir).generated_by.as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// What an earlier Konstruktor seeded follows the files; what somebody chose does not.
    #[test]
    fn a_move_brings_seeded_images_to_their_major_and_leaves_chosen_ones() {
        use crate::catalog::ServiceId::Rekuest;
        use crate::config::hub::{caught_up_image, is_supported_image};

        let seeded = config().rekuest.image.unwrap();
        for behind in [
            "jhnnsrs/rekuest:latest",
            "jhnnsrs/rekuest:latest@sha256:abc",
            "jhnnsrs/rekuest:5",
        ] {
            assert_eq!(caught_up_image(Rekuest, behind).as_ref(), Some(&seeded));
            assert!(!is_supported_image(Rekuest, behind));
        }
        for chosen in [
            "jhnnsrs/rekuest:5.2.0",
            "jhnnsrs/rekuest:next",
            "registry.lab/rekuest:latest",
            "next-rekuest",
        ] {
            assert_eq!(caught_up_image(Rekuest, chosen), None, "{chosen}");
            assert!(!is_supported_image(Rekuest, chosen), "{chosen}");
        }
        assert_eq!(caught_up_image(Rekuest, &seeded), None);
        assert!(is_supported_image(Rekuest, &seeded));
        assert!(is_supported_image(Rekuest, &format!("{seeded}.0.1")));

        let mut config = config();
        config.rekuest.image = Some("jhnnsrs/rekuest:latest".into());
        config.mikro.image = Some("jhnnsrs/mikro:next".into());
        assert_eq!(caught_up_images(&config), [("rekuest".to_string(), seeded)]);
        assert_eq!(
            unsupported_images(&config),
            [("mikro".to_string(), "jhnnsrs/mikro:next".to_string())]
        );
    }
}
