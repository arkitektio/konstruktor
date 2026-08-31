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
//! The mechanism is a profile rewrite. Generation reads `config.<service>.image`, so the
//! only way to put a container back on an older image is to write that image into the
//! profile and regenerate — the same sequence `create::reauthorize` performs, for the same
//! reason.

use std::path::Path;

use serde::Serialize;

use crate::config::hub::HubConfig;
use crate::lock::{self, Entry};
use crate::profile::read_profile;

#[derive(Debug, thiserror::Error)]
pub enum RollbackError {
    #[error("{0}")]
    Profile(String),
    #[error("there is no record of what this hub was running before — `hub_lock.json` is \
             written by `konstruktor update`, so a hub that has not been updated since \
             this existed has nothing to go back to")]
    NoHistory,
    #[error("every service is already on the image it would be rolled back to")]
    NothingToDo,
    #[error("{0}")]
    Write(#[from] std::io::Error),
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
}

/// The previous state, and what returning to it would change.
pub fn plan(dir: &Path) -> Result<RollbackPlan, RollbackError> {
    let config = read_profile(dir)
        .map(|profile| profile.config)
        .map_err(|e| RollbackError::Profile(e.to_string()))?;
    let history = lock::read(dir);
    let previous = history.previous().ok_or(RollbackError::NoHistory)?;

    let (changes, unrollable) = changes_against(&config, previous);
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
            if unrollable.len() == 1 { "it is" } else { "they are" },
            if unrollable.len() == 1 { "it" } else { "they" },
            if unrollable.len() == 1 { "is" } else { "are" },
        ));
    }

    Ok(RollbackPlan {
        recorded_at: previous.at,
        reason: previous.reason.clone(),
        changes,
        unrollable,
        warnings,
    })
}

/// Which services the recorded state would actually move, and which it cannot.
fn changes_against(config: &HubConfig, previous: &Entry) -> (Vec<Change>, Vec<String>) {
    let mut changes = Vec::new();
    let mut unrollable = Vec::new();

    for (service, current) in config.stack_images() {
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

/// Writes the older images into the profile and regenerates the deployment from it.
///
/// Recreating the containers is the caller's — nothing here starts or stops anything. The
/// write itself is [`crate::profile::rewrite_images`], shared with `update --infra`, which
/// moves images in the other direction.
pub fn apply(dir: &Path, plan: &RollbackPlan) -> Result<(), RollbackError> {
    let images: Vec<(String, String)> = plan
        .changes
        .iter()
        .map(|change| (change.service.clone(), change.to.clone()))
        .collect();
    crate::profile::rewrite_images(dir, &images).map_err(|e| RollbackError::Profile(e.to_string()))
}

/// Records the state a rollback landed on, so the file keeps describing what is running.
pub async fn record_applied(dir: &Path) -> Result<(), RollbackError> {
    let config = read_profile(dir)
        .map(|profile| profile.config)
        .map_err(|e| RollbackError::Profile(e.to_string()))?;
    lock::record(dir, &config, "rolled back", lock::now()).await?;
    Ok(())
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

        let (changes, unrollable) = changes_against(&config, &entry);
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
            }],
        };
        assert!(lock.previous().is_none());
    }
}
