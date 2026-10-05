//! Putting a hub back on the images it was running before its last update.
//!
//! **This reverts code, not data.** Every service here is `command: bash run.sh`, and
//! `run.sh` migrates the database forward when the container starts. So by the time an
//! update has gone wrong the schema has already moved, and putting the old image back
//! points last week's code at this week's database. That is often enough — a bad build, a
//! broken template, a service that will not boot — and it is never a substitute for the
//! backup `update` takes first. Both front ends have to say so before doing it; there is
//! no wording of this that makes it safe to leave unsaid.
//!
//! The mechanism is the lock. A hub runs the build written down for each service
//! ([`crate::pins`]), so going back is writing down the earlier build and generating the
//! files with it — the profile keeps naming the channel the service follows. And since the
//! next `update` would move it straight forward again, a service that was put back is
//! frozen ([`crate::freeze`]): `unfreeze`, then `update`, is how it moves on once whatever
//! was wrong with the newer release is fixed.
//!
//! Unless the update being undone moved the hub's files to another layout
//! ([`crate::migrate`]): then what is generated today is not what the older images read,
//! and the files go back too — from the copy `update` kept of them
//! ([`crate::generations`]), with the older images written into that.

use std::path::Path;

use serde::Serialize;

use crate::config::hub::HubConfig;
use crate::lock::{self, Entry};
use crate::profile::read_profile;

#[derive(Debug, thiserror::Error)]
pub enum RollbackError {
    #[error("{0}")]
    Profile(String),
    #[error(
        "there is no record of what this hub was running before — `hub_lock.json` is \
             written by `konstruktor update`, so a hub that has not been updated since \
             this existed has nothing to go back to"
    )]
    NoHistory,
    #[error("every service is already on the image it would be rolled back to")]
    NothingToDo,
    #[error("{0}")]
    Write(#[from] std::io::Error),
    #[error("{0}")]
    Compose(String),
}

/// One service moving from the image it runs to the one it ran before.
#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub service: String,
    pub from: String,
    pub to: String,
}

/// What a rollback would do, before anything is written.
#[derive(Debug, Clone, Serialize)]
pub struct RollbackPlan {
    /// When the state being returned to was recorded, and why it was.
    pub recorded_at: u64,
    pub reason: String,
    pub changes: Vec<Change>,
    /// Services the record cannot put back: nothing was ever pulled for them, or the
    /// image carries no registry digest, so there is no reference to return to. They are
    /// left exactly as they are rather than guessed at.
    pub unrollable: Vec<String>,
    /// Said whatever the plan holds — see this module's own warning about migrations.
    pub warnings: Vec<String>,
    /// The kept copy of the hub's files that goes back with the images: set when the
    /// state returned to ran on files of another layout than the ones on disk.
    pub files: Option<String>,
    /// The backup of the data taken when the state returned to was recorded: what puts
    /// the database back, which this does not.
    pub backup: Option<String>,
}

/// The previous state, and what returning to it would change.
pub fn plan(dir: &Path) -> Result<RollbackPlan, RollbackError> {
    let config = read_profile(dir)
        .map(|profile| profile.config)
        .map_err(|e| RollbackError::Profile(e.to_string()))?;
    let history = lock::read(dir);
    let previous = history.previous().ok_or(RollbackError::NoHistory)?;

    let (changes, unrollable) = changes_against(&config, &history.pins, previous);
    if changes.is_empty() {
        return Err(RollbackError::NothingToDo);
    }

    let mut warnings = vec![
        "this puts the images back; it does not put the database back. Migrations run \
         when a service starts and are one-way, so the older code will be talking to the \
         newer schema — restore the backup taken before the update if that is not enough"
            .to_string(),
    ];
    if !unrollable.is_empty() {
        warnings.push(format!(
            "no earlier image was recorded for {} — {} left as {} {}",
            unrollable.join(", "),
            if unrollable.len() == 1 {
                "it is"
            } else {
                "they are"
            },
            if unrollable.len() == 1 { "it" } else { "they" },
            if unrollable.len() == 1 { "is" } else { "are" },
        ));
    }

    let files = previous.files.clone().filter(|name| {
        crate::generations::layout(dir, name, &config)
            .is_some_and(|kept| kept != crate::migrate::layout(dir, &config))
    });
    warnings.push(
        "what is put back is frozen, or the next update would move it forward again: \
         `konstruktor unfreeze`, then `konstruktor update`, when it should move on"
            .to_string(),
    );
    if files.is_some() {
        warnings.push(
            "the update being undone rewrote this hub's files for the newer releases; the \
             files it kept from before are put back with the images"
                .to_string(),
        );
    }

    Ok(RollbackPlan {
        recorded_at: previous.at,
        reason: previous.reason.clone(),
        changes,
        unrollable,
        warnings,
        files,
        backup: previous.backup.clone(),
    })
}

/// Which services the recorded state would actually move, and which it cannot.
fn changes_against(
    config: &HubConfig,
    pins: &std::collections::BTreeMap<String, lock::Pin>,
    previous: &Entry,
) -> (Vec<Change>, Vec<String>) {
    let mut changes = Vec::new();
    let mut unrollable = Vec::new();
    // What each service runs now: the build written down for it, or — with none — what
    // the profile names.
    let runs = crate::pins::references(config, pins);

    for (service, named) in config.stack_images() {
        let current = runs.get(&service).cloned().unwrap_or(named);
        let Some(pin) = previous.services.get(&service) else {
            continue;
        };
        match pin.reference() {
            // A pin with no digest is a service nothing was ever pulled for, or one built
            // locally. If it names the reference the profile already carries there is
            // simply nothing to move; otherwise the record cannot say what to move it to,
            // and guessing at the floating tag would change nothing while claiming
            // something.
            None if pin.image == current => {}
            None => unrollable.push(service),
            Some(reference) if reference != current => changes.push(Change {
                service,
                from: current,
                to: reference,
            }),
            Some(_) => {}
        }
    }
    (changes, unrollable)
}

/// Writes the earlier builds down and generates the hub's files with them — or, when the
/// plan names a kept copy of the files, puts that back and writes the images into it — and
/// freezes what was put back.
///
/// Recreating the containers is the caller's — nothing here starts or stops anything.
pub fn apply(dir: &Path, plan: &RollbackPlan) -> Result<(), RollbackError> {
    let moved: Vec<String> = plan
        .changes
        .iter()
        .map(|change| change.service.clone())
        .collect();
    match &plan.files {
        Some(name) => {
            let images: Vec<(String, String)> = plan
                .changes
                .iter()
                .map(|change| (change.service.clone(), change.to.clone()))
                .collect();
            restore_files(dir, name, &images)?;
        }
        None => pin_back(dir, plan)?,
    }
    crate::freeze::hold_exactly(dir, &moved)?;
    Ok(())
}

/// Writes each change's earlier build into the lock as the service's pin, under the
/// channel it was a build of, and regenerates. A profile that names something else for the
/// service — a channel that moved since, a digest an earlier rollback left there — is put
/// back on that channel.
fn pin_back(dir: &Path, plan: &RollbackPlan) -> Result<(), RollbackError> {
    let config = read_profile(dir)
        .map(|profile| profile.config)
        .map_err(|e| RollbackError::Profile(e.to_string()))?;
    let named: std::collections::BTreeMap<String, String> =
        config.stack_images().into_iter().collect();
    let mut channels: Vec<(String, String)> = Vec::new();
    let mut held = lock::read(dir);
    for change in &plan.changes {
        let Some((channel, digest)) = change.to.split_once('@') else {
            continue;
        };
        if named.get(&change.service).map(String::as_str) != Some(channel) {
            channels.push((change.service.clone(), channel.to_string()));
        }
        held.pins.insert(
            change.service.clone(),
            lock::Pin {
                image: channel.to_string(),
                digest: Some(digest.to_string()),
            },
        );
    }
    lock::write(dir, &held)?;
    crate::profile::rewrite_images(dir, &channels)
        .map_err(|e| RollbackError::Profile(e.to_string()))
}

/// Puts a kept copy of the files back and points it at `images`. Nothing is generated: the
/// files are of a layout this build no longer writes, so the images go into the profile
/// and into the compose file as they stand.
fn restore_files(dir: &Path, name: &str, images: &[(String, String)]) -> Result<(), RollbackError> {
    crate::generations::restore(dir, name)?;
    let mut profile = read_profile(dir).map_err(|e| RollbackError::Profile(e.to_string()))?;
    for (service, image) in images {
        profile.config.set_service_image(service, image);
    }

    let text = crate::compose_file::read(dir).map_err(|e| RollbackError::Profile(e.to_string()))?;
    let mut compose: serde_norway::Value =
        serde_norway::from_str(&text).map_err(|e| RollbackError::Profile(e.to_string()))?;
    // The reaper of a hub from before takt runs Rekuest's image.
    let reaper = crate::generate::compose::legacy_reaper_host(&profile.config);
    let rekuest = profile.config.rekuest.host.clone();
    if let Some(services) = compose
        .get_mut("services")
        .and_then(|services| services.as_mapping_mut())
    {
        for (service, image) in images {
            let follows = (service == &rekuest).then_some(reaper.as_str());
            for name in std::iter::once(service.as_str()).chain(follows) {
                if let Some(entry) = services.get_mut(name).and_then(|s| s.as_mapping_mut()) {
                    entry.insert("image".into(), image.as_str().into());
                }
            }
        }
    }
    let text = serde_norway::to_string(&compose).expect("a compose file always serializes");

    crate::profile::write_profile(dir, &profile)
        .map_err(|e| RollbackError::Profile(e.to_string()))?;
    crate::compose_file::write(dir, &text).map_err(|e| RollbackError::Profile(e.to_string()))
}

/// Records the state a rollback landed on, so the file keeps describing what is running.
pub async fn record_applied(dir: &Path) -> Result<(), RollbackError> {
    let config = read_profile(dir)
        .map(|profile| profile.config)
        .map_err(|e| RollbackError::Profile(e.to_string()))?;
    lock::record(dir, &config, "rolled back", lock::now()).await?;
    Ok(())
}

/// The whole rollback, as both front ends run it: rewrite the profile, fetch and recreate
/// each service that moves — `--no-deps`, nothing else is touched — and record the state
/// it landed on.
pub async fn run(
    dir: &Path,
    plan: &RollbackPlan,
    on_line: &(dyn Fn(crate::compose::ComposeLine) + Send + Sync),
) -> Result<(), RollbackError> {
    apply(dir, plan)?;
    let config = crate::profile::read_profile(dir).ok().map(|p| p.config);
    // Of the files as they are now: a restored copy may not name a service the plan moves
    // (takt, on a hub going back to before it).
    let declared = |service: &str| crate::compose_file::declares_service(dir, service);
    for change in plan.changes.iter().filter(|c| declared(&c.service)) {
        // takt moves back with Rekuest. Its own image is a change of its own in the plan
        // when it moved; recreating it here as well keeps the pair on one release even
        // when only Rekuest's did.
        let companions = config
            .as_ref()
            .map(|c| crate::generate::compose::companions(c, &change.service))
            .unwrap_or_default();
        let argvs = [
            crate::compose::pull_service(&change.service),
            crate::compose::up_service(&change.service),
        ]
        .into_iter()
        .chain(companions.iter().filter(|c| declared(c)).flat_map(|c| {
            [
                crate::compose::pull_service(c),
                crate::compose::up_service(c),
            ]
        }));
        for argv in argvs {
            crate::compose::run_streamed(dir, argv, on_line)
                .await
                .map_err(RollbackError::Compose)?;
        }
    }
    // Files that went back changed more than images: a service they no longer name has to
    // go, one they name again has to start, and everything reads its config anew.
    if let (Some(_), Some(config)) = (&plan.files, &config) {
        crate::services::apply_services(
            dir,
            &crate::services::every_config_reader(config),
            on_line,
        )
        .await
        .map_err(|error| RollbackError::Compose(error.to_string()))?;
    }
    record_applied(dir).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::hub::{build_hub_config, HubConfigOptions};
    use crate::lock::Pin;
    use std::collections::BTreeMap;

    fn config() -> HubConfig {
        build_hub_config(&HubConfigOptions {
            device_id: "device".into(),
            coord_server: "go.arkitekt.live".into(),
            ..Default::default()
        })
    }

    fn previous(pins: &[(&str, &str, Option<&str>)]) -> Entry {
        Entry {
            at: 1,
            reason: "before update".into(),
            services: pins
                .iter()
                .map(|(service, image, digest)| {
                    (
                        service.to_string(),
                        Pin {
                            image: image.to_string(),
                            digest: digest.map(str::to_string),
                        },
                    )
                })
                .collect(),
            files: None,
            backup: None,
        }
    }

    /// The image a service is on now is what it is compared against, so a hub already back
    /// on the older image has nothing to do — and a service with no recorded digest is
    /// reported rather than quietly skipped.
    #[test]
    fn only_services_with_an_older_image_move() {
        let mut config = config();
        // Already digest-pinned here — a hub that has been rolled back once before — while
        // the record for it names only the floating tag, with nothing pulled behind it.
        config.mikro.image = Some("jhnnsrs/mikro:next@sha256:new".into());
        let entry = previous(&[
            ("rekuest", "jhnnsrs/rekuest:next", Some("sha256:old")),
            // Same reference it is on now: nothing to do.
            ("gateway", &config.gateway.image, None),
            // Never pulled, so there is no earlier image to return to.
            ("mikro", "jhnnsrs/mikro:next", None),
        ]);

        let (changes, unrollable) = changes_against(&config, &BTreeMap::new(), &entry);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].service, "rekuest");
        assert_eq!(changes[0].to, "jhnnsrs/rekuest:next@sha256:old");
        // The gateway's recorded pin carries no digest either, but it names the image the
        // profile already has — nothing to move, and nothing to report.
        assert_eq!(unrollable, vec!["mikro".to_string()]);
    }

    /// Every compose service the stack declares has to be reachable, or a rollback would
    /// report a change it did not make.
    #[test]
    fn every_service_in_the_stack_can_be_written_back() {
        let mut config = config();
        for (service, _) in config.clone().stack_images() {
            config.set_service_image(&service, "pinned@sha256:abc");
        }
        for (service, image) in config.stack_images() {
            assert_eq!(image, "pinned@sha256:abc", "{service} was not written back");
        }
    }

    /// The whole point of the file: an entry the profile does not mention is ignored, and
    /// a hub with one entry has nowhere to go.
    #[test]
    fn a_hub_with_no_earlier_state_is_refused() {
        let lock = crate::lock::Lock {
            version: 1,
            history: vec![Entry {
                at: 1,
                reason: "updated".into(),
                services: BTreeMap::new(),
                files: None,
                backup: None,
            }],
            ..Default::default()
        };
        assert!(lock.previous().is_none());
    }

    /// An update that moved the files to another layout is undone with the files it kept:
    /// what is generated today is not what the older images read.
    #[test]
    fn files_of_another_layout_go_back_with_the_images() {
        let dir = std::env::temp_dir().join(format!(
            "konstruktor-rollback-files-{}-{}",
            std::process::id(),
            lock::now()
        ));
        std::fs::create_dir_all(dir.join("configs")).unwrap();
        let config = config();
        crate::profile::write_profile(&dir, &crate::profile::hub_profile(config.clone())).unwrap();
        // A hub from before takt, as it was when its update began.
        let then = "services:\n  rekuest:\n    image: jhnnsrs/rekuest:latest\n  rekuest-reaper:\n    image: jhnnsrs/rekuest:latest\n  mikro:\n    image: jhnnsrs/mikro:latest\n";
        std::fs::write(dir.join("docker-compose.yaml"), then).unwrap();
        std::fs::write(dir.join("configs/rekuest.yaml"), "then: true\n").unwrap();
        let mut held = lock::read(&dir);
        held.history.push(Entry {
            files: Some(crate::generations::take(&dir, 1).unwrap()),
            ..previous(&[("rekuest", "jhnnsrs/rekuest:latest", Some("sha256:old"))])
        });
        lock::write(&dir, &held).unwrap();

        // The update: files of today, and the lock moves on.
        crate::profile::rewrite(&dir, config.clone(), &[]).unwrap();
        let mut held = lock::read(&dir);
        held.history.push(Entry {
            reason: "updated".into(),
            ..previous(&[("rekuest", "jhnnsrs/rekuest:latest", Some("sha256:new"))])
        });
        lock::write(&dir, &held).unwrap();
        assert!(crate::compose_file::declares_service(&dir, "rekuest-takt"));

        let plan = plan(&dir).unwrap();
        assert_eq!(plan.files.as_deref(), Some("1"));
        apply(&dir, &plan).unwrap();

        assert_eq!(crate::migrate::layout(&dir, &config), 1);
        assert!(!crate::compose_file::declares_service(&dir, "rekuest-takt"));
        assert_eq!(
            std::fs::read_to_string(dir.join("configs/rekuest.yaml")).unwrap(),
            "then: true\n"
        );
        // The older image, in the profile and in both services that run it.
        let compose = std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap();
        assert_eq!(
            compose
                .matches("image: jhnnsrs/rekuest:latest@sha256:old")
                .count(),
            2,
            "{compose}"
        );
        assert_eq!(
            read_profile(&dir).unwrap().config.rekuest.image.as_deref(),
            Some("jhnnsrs/rekuest:latest@sha256:old")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Going back is writing the earlier build down: the profile keeps its channel, the
    /// compose file names the build, and what was put back is frozen so that the next
    /// update does not undo the rollback.
    #[test]
    fn an_earlier_build_is_written_down_and_held() {
        let dir = std::env::temp_dir().join(format!(
            "konstruktor-rollback-pins-{}-{}",
            std::process::id(),
            lock::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config = config();
        let channel = config.rekuest.image.clone().unwrap();
        crate::profile::rewrite(&dir, config.clone(), &[]).unwrap();
        // It ran `old`, was updated to `new`, and runs `new` now.
        let mut held = lock::read(&dir);
        held.history
            .push(previous(&[("rekuest", &channel, Some("sha256:old"))]));
        held.history.push(Entry {
            reason: "updated".into(),
            ..previous(&[("rekuest", &channel, Some("sha256:new"))])
        });
        held.pins.insert(
            "rekuest".into(),
            Pin {
                image: channel.clone(),
                digest: Some("sha256:new".into()),
            },
        );
        lock::write(&dir, &held).unwrap();
        crate::profile::rewrite(&dir, config.clone(), &[]).unwrap();

        let plan = plan(&dir).unwrap();
        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.changes[0].from, format!("{channel}@sha256:new"));
        assert_eq!(plan.changes[0].to, format!("{channel}@sha256:old"));
        apply(&dir, &plan).unwrap();

        assert_eq!(
            read_profile(&dir).unwrap().config.rekuest.image.as_deref(),
            Some(channel.as_str()),
            "the profile keeps the channel"
        );
        let compose = std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap();
        assert!(
            compose.contains(&format!("image: {channel}@sha256:old\n")),
            "{compose}"
        );
        assert_eq!(
            crate::freeze::frozen(&dir).keys().collect::<Vec<_>>(),
            ["rekuest"]
        );
        // Where it is now is where the record says it was: nothing left to put back.
        assert!(matches!(super::plan(&dir), Err(RollbackError::NothingToDo)));
        std::fs::remove_dir_all(&dir).ok();
    }
}
