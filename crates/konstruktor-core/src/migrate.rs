//! Which layout a hub's generated files have, and what takes them to the current one.
//!
//! A hub keeps running the files it was last given. What the generator writes changes —
//! a service it gained (takt), a key a service renamed — and the images a hub follows
//! change with it, so files and images have to move together. For that a hub has to be
//! able to say which files it has: the layout number in `hub_lock.json`, bumped whenever
//! files of the older number would not run the images of the newer.
//!
//! A move is mostly a regeneration: the profile holds everything the files are made from.
//! `konstruktor update` is the one path across — it regenerates and moves every service,
//! since files of a new layout and images of an old one are exactly the hub that comes up
//! healthy and does nothing. Everything else that rewrites a hub's files asks [`behind`]
//! first and refuses.
//!
//! What regeneration cannot say is a [`Step`]'s [`Action`]s: docker commands run once, in
//! the hub's folder, when the hub crosses that step — a volume copied, a one-off container
//! run, something removed that the new files no longer name. They are written here, beside
//! the layout they belong to ([`steps`]), and come in two kinds:
//!
//! - [`When::Before`] runs when everything is ready — files rewritten, images fetched,
//!   each release asked whether it reads its config — and before the first container is
//!   replaced. If it fails the update stops, the files go back, and the hub keeps running
//!   as it was. So it has to be harmless to have run on a hub that then stays where it is:
//!   additive, and repeatable.
//! - [`When::After`] runs once everything is recreated on the new files. A failure there
//!   is said, not undone; the step is written down as unfinished, the next update runs its
//!   closing commands again before anything else, and nothing else rewrites the hub's
//!   files until they have run.
//!
//! What a *service* has to do to its own data when its version changes is not here: that
//! is the service's, shipped in its image: its `migrate` job, run by `updates`.

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
/// 5. The compose file names the build of every image ([`crate::pins`]).
/// 6. Every service's image writes its own config and prepares its own database
///    ([`crate::contract`]), on the majors that do: Rekuest 7, Mikro 7, Kabinet 6,
///    Elektro 5, Alpaka 5, Fluss 4, Bank 4, Kuvert 4, Lovekit 3, Lokate 3, Kraph 2,
///    Dokuments 2.
/// 7. Bank 5 and Kuvert 5: their providers and logins are set up in the client, not in
///    the config.
pub const CURRENT_LAYOUT: u32 = 7;

/// The newest layout written before layouts were recorded: what a hub with no record is
/// taken for unless its files say otherwise.
const LAST_UNRECORDED: u32 = 3;

/// When, in an update, an [`Action`] runs. See this module's introduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum When {
    Before,
    After,
}

/// One docker command a [`Step`] needs run, once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Action {
    pub when: When,
    /// What it does, as a front end says it before and while it runs.
    pub title: String,
    /// The arguments to the container engine, run in the hub's folder:
    /// `["compose", "run", "--rm", …]`, `["volume", "create", …]`.
    pub docker: Vec<String>,
}

/// One move from a layout to the next, as a front end shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub to: u32,
    pub title: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Action>,
}

impl Step {
    fn new(to: u32, title: &str) -> Self {
        Step {
            to,
            title: title.to_string(),
            actions: Vec::new(),
        }
    }

    /// The commands of this step that run at `when`, in order.
    pub fn at(&self, when: When) -> impl Iterator<Item = &Action> {
        self.actions
            .iter()
            .filter(move |action| action.when == when)
    }
}

/// Every move this build knows, in order: the layout it leads to, what changes, and the
/// commands it needs run. A new layout is a new entry here and a new [`CURRENT_LAYOUT`].
///
/// None needs a command so far. Rekuest's reaper, which layout 2 drops, goes with the
/// `up --remove-orphans` every update ends with.
pub fn steps() -> Vec<Step> {
    vec![
        Step::new(2, "takt runs beside Rekuest, in place of its reaper"),
        Step::new(
            3,
            "Rekuest and takt share a socket, and Rekuest knows its services and hook agents apart",
        ),
        Step::new(
            4,
            "every service follows the major release these files are written for, not `latest`",
        ),
        Step::new(
            5,
            "the compose file names the exact build of every image, and only an update moves it",
        ),
        Step::new(
            6,
            "every service writes its own config and prepares its own database, on the releases that do",
        ),
        Step::new(
            7,
            "Bank and Kuvert follow their 5 releases: providers and logins are set up in the client",
        ),
    ]
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
    if service(&config.rekuest().host).is_none() {
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
    pending_of(&steps(), layout(dir, config))
}

/// [`pending`], of any list of steps: those past `layout`.
pub fn pending_of(steps: &[Step], layout: u32) -> Vec<Step> {
    steps
        .iter()
        .filter(|step| step.to > layout)
        .cloned()
        .collect()
}

/// The steps whose files are written and whose closing commands have not all run.
pub fn unfinished(dir: &Path) -> Vec<Step> {
    let recorded = lock::read(dir).unfinished;
    steps()
        .into_iter()
        .filter(|step| recorded.contains(&step.to))
        .collect()
}

/// Writes down which of `steps` still have closing commands to run: at the point an update
/// starts replacing containers, from where there is no going back to the old files.
pub fn begin(dir: &Path, steps: &[Step]) -> std::io::Result<()> {
    let closing: Vec<u32> = steps
        .iter()
        .filter(|step| step.at(When::After).next().is_some())
        .map(|step| step.to)
        .collect();
    if closing.is_empty() {
        return Ok(());
    }
    let mut held = lock::read(dir);
    for to in closing {
        if !held.unfinished.contains(&to) {
            held.unfinished.push(to);
        }
    }
    lock::write(dir, &held)
}

/// A step's closing commands have run.
pub fn finish(dir: &Path, to: u32) -> std::io::Result<()> {
    let mut held = lock::read(dir);
    let before = held.unfinished.len();
    held.unfinished.retain(|step| *step != to);
    if held.unfinished.len() == before {
        return Ok(());
    }
    lock::write(dir, &held)
}

/// Why this hub's files must not be rewritten in passing, if they must not: they are of an
/// older layout — only an update moves the images along with them — or a move is half
/// done, with commands still to run that the files as they are wait for.
pub fn behind(dir: &Path, config: &HubConfig) -> Option<String> {
    let waiting = unfinished(dir);
    if !waiting.is_empty() {
        return Some(format!(
            "this hub's last update did not finish ({}): commands it still has to run \
             failed. Run `konstruktor update` again, which runs them first. Nothing was \
             changed.",
            waiting
                .iter()
                .map(|step| step.title.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
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
    config
        .service_ids()
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
/// hash. The compose file is written with the builds the lock holds for this profile
/// ([`crate::pins`]), so every path that regenerates keeps the hub on them without knowing.
/// A service's own config is not written here: its image writes it ([`crate::contract`]).
/// A file the record held and the generator no longer writes is removed.
pub fn write_hub(dir: &Path, config: &HubConfig, files: &GeneratedFiles) -> std::io::Result<()> {
    let held = lock::read(dir);
    let mut written: BTreeMap<String, String> = files.clone();
    crate::pins::apply(&mut written, config, &held.pins);
    crate::generate::write::write_generated_files(dir, &written)?;
    // The services' configs are theirs, written by their images and recorded with them:
    // they stay on the record while their service runs, and go when it does.
    let running: Vec<String> = config
        .enabled_services()
        .into_iter()
        .map(|id| format!("configs/{}.yaml", config.service(id).host))
        .collect();
    let stale = lock::stamp(dir, CURRENT_LAYOUT, &written)?;
    let mut now = lock::read(dir);
    for path in stale {
        match (running.contains(&path), held.files.get(&path)) {
            (true, Some(hash)) => {
                now.files.insert(path, hash.clone());
            }
            _ => {
                let _ = std::fs::remove_file(dir.join(path));
            }
        }
    }
    lock::write(dir, &now)
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
        assert_eq!(pending(&dir, &config).len(), 6);
        assert!(behind(&dir, &config)
            .unwrap()
            .contains("konstruktor update"));

        write("services:\n  rekuest: {}\n  rekuest-takt: {}\n");
        assert_eq!(layout(&dir, &config), 2);

        write(
            "services:\n  rekuest: {}\n  rekuest-takt:\n    environment:\n      TAKT_INTERNAL_BIND: unix:/run/takt/internal.sock\n",
        );
        assert_eq!(layout(&dir, &config), 3);
        assert_eq!(pending(&dir, &config).len(), 4);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Once written by this build, the record answers — whatever the files look like.
    #[test]
    fn written_files_are_recorded_and_a_file_no_longer_generated_goes() {
        let config = config();
        let dir = scratch("write");
        let files =
            crate::generate::generate_hub_files(&config, &Default::default(), &Default::default());
        let mut with_extra = files.clone();
        with_extra.insert("configs/gone.yaml".into(), "x: 1\n".into());

        write_hub(&dir, &config, &with_extra).unwrap();
        assert!(dir.join("configs/gone.yaml").exists());
        write_hub(&dir, &config, &files).unwrap();
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
        use crate::catalog::ServiceId;
        use crate::config::hub::{caught_up_image, is_supported_image};

        let seeded = config().rekuest().image.clone().unwrap();
        for behind in [
            "jhnnsrs/rekuest:latest",
            "jhnnsrs/rekuest:latest@sha256:abc",
            "jhnnsrs/rekuest:5",
        ] {
            assert_eq!(
                caught_up_image(ServiceId::Rekuest, behind).as_ref(),
                Some(&seeded)
            );
            assert!(!is_supported_image(ServiceId::Rekuest, behind));
        }
        for chosen in [
            "jhnnsrs/rekuest:5.2.0",
            "jhnnsrs/rekuest:next",
            "registry.lab/rekuest:latest",
            "next-rekuest",
        ] {
            assert_eq!(
                caught_up_image(ServiceId::Rekuest, chosen),
                None,
                "{chosen}"
            );
            assert!(!is_supported_image(ServiceId::Rekuest, chosen), "{chosen}");
        }
        assert_eq!(caught_up_image(ServiceId::Rekuest, &seeded), None);
        assert!(is_supported_image(ServiceId::Rekuest, &seeded));
        assert!(is_supported_image(
            ServiceId::Rekuest,
            &format!("{seeded}.0.1")
        ));

        let mut config = config();
        config.service_mut(crate::catalog::ServiceId::Rekuest).image =
            Some("jhnnsrs/rekuest:latest".into());
        config.service_mut(crate::catalog::ServiceId::Mikro).image =
            Some("jhnnsrs/mikro:next".into());
        assert_eq!(caught_up_images(&config), [("rekuest".to_string(), seeded)]);
        assert_eq!(
            unsupported_images(&config),
            [("mikro".to_string(), "jhnnsrs/mikro:next".to_string())]
        );
    }

    /// A step's closing commands are owed from the moment containers are replaced until
    /// they have run, and the hub is not rewritten by anything else meanwhile.
    #[test]
    fn a_step_with_closing_commands_is_unfinished_until_they_ran() {
        let config = config();
        let dir = scratch("unfinished");
        crate::profile::rewrite(&dir, config.clone(), &[]).unwrap();
        assert_eq!(behind(&dir, &config), None);

        let action = |when| Action {
            when,
            title: "copy the volume".into(),
            docker: vec!["volume".into(), "ls".into()],
        };
        let mut plain = Step::new(4, "nothing to run");
        let mut closing = Step::new(5, "has something to run afterwards");
        plain.actions.push(action(When::Before));
        closing.actions.push(action(When::Before));
        closing.actions.push(action(When::After));
        assert_eq!(
            pending_of(&[plain.clone(), closing.clone()], 4),
            [closing.clone()]
        );
        assert_eq!(closing.at(When::After).count(), 1);

        begin(&dir, &[plain, closing]).unwrap();
        assert_eq!(lock::read(&dir).unfinished, [5]);
        assert_eq!(unfinished(&dir).len(), 1);
        assert!(behind(&dir, &config).unwrap().contains("did not finish"));
        assert!(matches!(
            crate::profile::rewrite_images(&dir, &[]),
            Err(crate::profile::ProfileError::Layout(_))
        ));

        finish(&dir, 5).unwrap();
        assert_eq!(behind(&dir, &config), None);
        crate::profile::rewrite_images(&dir, &[]).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }
}
