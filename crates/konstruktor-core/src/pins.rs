//! The exact build of every image a hub runs, written into its compose file.
//!
//! The profile names channels — `jhnnsrs/rekuest:6`, a tag that moves with every release
//! of that major. Which build a tag means depends on when a machine last fetched it, so a
//! compose file that names tags runs whatever that happens to be: something else after a
//! `docker compose pull`, an image prune, a restore onto another machine.
//!
//! So the three files say three things. The **profile** is intent: the channel a service
//! follows, never a digest. The **lock** is fact: the digest each channel resolved to
//! when it was last looked at ([`Lock::pins`](crate::lock::Lock)). The **compose file** is
//! written from both: `repo:tag@sha256:…`. Starting, restarting or copying a hub then runs
//! those builds, and `update` is the one thing that looks again.
//!
//! A pin belongs to a channel: once the profile names another image for a service, the
//! pin no longer counts, and the service runs its tag until it is resolved again. An image
//! with no registry digest — built on this machine, never pulled — has no pin and keeps
//! its tag.

use std::collections::BTreeMap;
use std::path::Path;

use crate::compose_file::COMPOSE_FILENAME;
use crate::config::hub::HubConfig;
use crate::generate::GeneratedFiles;
use crate::lock::{self, Pin};

/// The digest `image`'s repository was pulled as, from an engine's `RepoDigests`.
fn digest_for(image: &str, repo_digests: &[String]) -> Option<String> {
    let named = image.split('@').next().unwrap_or(image);
    let repository = named
        .rsplit_once(':')
        .map_or(named, |(repository, _)| repository);
    repo_digests
        .iter()
        .filter_map(|entry| entry.rsplit_once('@'))
        .find(|(found, _)| *found == repository)
        .map(|(_, digest)| digest.to_string())
}

/// The pins that count for this profile, as the reference each service is written with:
/// those whose channel is still the one the profile names. A service whose profile image
/// names a digest itself (a rollback wrote it) is already exact and has none.
pub fn references(config: &HubConfig, pins: &BTreeMap<String, Pin>) -> BTreeMap<String, String> {
    config
        .stack_images()
        .into_iter()
        .filter(|(_, image)| !image.contains('@'))
        .filter_map(|(service, image)| {
            let pin = pins.get(&service).filter(|pin| pin.image == image)?;
            Some((service, format!("{image}@{}", pin.digest.as_ref()?)))
        })
        .collect()
}

/// The services of the stack that are not on an exact build yet.
pub fn unpinned(config: &HubConfig, pins: &BTreeMap<String, Pin>) -> Vec<(String, String)> {
    let pinned = references(config, pins);
    config
        .stack_images()
        .into_iter()
        .filter(|(service, image)| !image.contains('@') && !pinned.contains_key(service))
        .collect()
}

/// Writes the pins into the generated compose document: each pinned service's `image`
/// becomes `repo:tag@sha256:…`. Everything else is left as generated.
pub fn apply(files: &mut GeneratedFiles, config: &HubConfig, pins: &BTreeMap<String, Pin>) {
    let references = references(config, pins);
    if references.is_empty() {
        return;
    }
    let Some(text) = files.get(COMPOSE_FILENAME) else {
        return;
    };
    let Ok(mut compose) = serde_norway::from_str::<serde_norway::Value>(text) else {
        return;
    };
    let Some(services) = compose
        .get_mut("services")
        .and_then(|services| services.as_mapping_mut())
    else {
        return;
    };
    for (service, reference) in &references {
        if let Some(entry) = services
            .get_mut(service.as_str())
            .and_then(|entry| entry.as_mapping_mut())
        {
            entry.insert("image".into(), reference.as_str().into());
        }
    }
    files.insert(
        COMPOSE_FILENAME.to_string(),
        crate::generate::dump(&compose),
    );
}

/// What each of `images` (compose service, reference) resolves to on this machine now.
/// A service whose image has no registry digest is not in the answer.
pub async fn resolve(images: &[(String, String)]) -> BTreeMap<String, Pin> {
    crate::docker::image_states(images)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|state| {
            let digest = digest_for(&state.image, &state.repo_digests)?;
            Some((
                state.service,
                Pin {
                    image: state.image,
                    digest: Some(digest),
                },
            ))
        })
        .collect()
}

/// The build each of `images` *runs*: the image its container was created from, and only
/// where there is no container what its tag resolves to. For a hub that ran before its
/// builds were written down — a tag may have been fetched further since, and what is
/// pinned has to be what runs.
pub async fn adopt(dir: &Path, images: &[(String, String)]) -> BTreeMap<String, Pin> {
    let engine = crate::engine_probe::engine();
    let listed = engine
        .async_command()
        .args(["compose", "ps", "--all", "--format", "{{.Service}}|{{.ID}}"])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .await;
    let mut created_from: Vec<(String, String)> = Vec::new();
    if let Ok(listed) = listed {
        for line in String::from_utf8_lossy(&listed.stdout).lines() {
            let Some((service, container)) = line.split_once('|') else {
                continue;
            };
            let inspected = engine
                .async_command()
                .args(["inspect", "--format", "{{.Image}}", container.trim()])
                .stdin(std::process::Stdio::null())
                .output()
                .await;
            if let Ok(out) = inspected {
                let image_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if out.status.success() && !image_id.is_empty() {
                    created_from.push((service.trim().to_string(), image_id));
                }
            }
        }
    }
    let running: BTreeMap<String, Vec<String>> = crate::docker::image_states(&created_from)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|state| (state.service, state.repo_digests))
        .collect();

    let mut pins = resolve(images).await;
    for (service, image) in images {
        if let Some(digest) = running
            .get(service)
            .and_then(|digests| digest_for(image, digests))
        {
            pins.insert(
                service.clone(),
                Pin {
                    image: image.clone(),
                    digest: Some(digest),
                },
            );
        }
    }
    pins
}

/// Writes builds into a compose file as it stands: every `image: <channel>` line of a
/// service in `references` becomes `image: <channel>@<digest>`. By line, not by parsing:
/// this is the one generated file people edit, and their comments and order stay.
pub fn write_into(compose: &str, channels: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(compose.len() + channels.len() * 80);
    for line in compose.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let named = body
            .trim_start()
            .strip_prefix("image:")
            .map(|image| image.trim().trim_matches(['"', '\'']));
        match named.and_then(|image| channels.get(image)) {
            Some(reference) => {
                let indent = &body[..body.len() - body.trim_start().len()];
                out.push_str(&format!(
                    "{indent}image: {reference}{}",
                    &line[body.len()..]
                ));
            }
            None => out.push_str(line),
        }
    }
    out
}

/// Before a hub is started: every image that has no build written down yet is fetched
/// and pinned — so that the compose file a container is created
/// from already names its build, and never changes under it afterwards. A new hub, a
/// service just added, a channel that changed.
///
/// Only the compose file's `image:` lines are touched. An image that cannot be fetched or
/// has no registry digest (built here) is left on its tag: `up` says what is wrong with it,
/// if anything is. Returns the services pinned.
pub async fn pin_before_start(
    dir: &Path,
    config: &HubConfig,
    on_line: &(dyn Fn(crate::compose::ComposeLine) + Send + Sync),
) -> Vec<String> {
    let mut held = lock::read(dir);
    let missing = unpinned(config, &held.pins);
    if missing.is_empty() {
        return Vec::new();
    }
    let states = crate::docker::image_states(&missing)
        .await
        .unwrap_or_default();
    // Fetched once, here: what is written down is what the channel points at now, not
    // whatever this machine happened to have of it. An image that is here and came from no
    // registry (built on this machine) is not asked for anywhere.
    for state in states
        .iter()
        .filter(|state| !state.present || !state.repo_digests.is_empty())
    {
        let pull = vec!["pull".to_string(), state.image.clone()];
        let _ = crate::compose::run_streamed(dir, pull, on_line).await;
    }
    let found = resolve(&missing).await;
    if found.is_empty() {
        return Vec::new();
    }
    let channels: BTreeMap<String, String> = found
        .values()
        .filter_map(|pin| Some((pin.image.clone(), pin.reference()?)))
        .collect();
    let path = dir.join(COMPOSE_FILENAME);
    let Ok(before) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let after = write_into(&before, &channels);
    if after != before && std::fs::write(&path, &after).is_err() {
        return Vec::new();
    }
    // The file stays "as generated" to whoever asks whether it was edited by hand —
    // unless it already was.
    if held.files.get(COMPOSE_FILENAME) == Some(&lock::digest(before.as_bytes())) {
        held.files
            .insert(COMPOSE_FILENAME.to_string(), lock::digest(after.as_bytes()));
    }
    let pinned: Vec<String> = found.keys().cloned().collect();
    held.version = 1;
    held.pins.extend(found);
    let _ = lock::write(dir, &held);
    pinned
}

/// Writes `pins` into the lock, beside the ones it holds for other services.
pub fn record(dir: &Path, pins: BTreeMap<String, Pin>) -> std::io::Result<()> {
    if pins.is_empty() {
        return Ok(());
    }
    let mut held = lock::read(dir);
    held.version = 1;
    held.pins.extend(pins);
    lock::write(dir, &held)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::hub::{build_hub_config, HubConfigOptions};

    fn config() -> HubConfig {
        build_hub_config(&HubConfigOptions::default())
    }

    fn pin(image: &str, digest: &str) -> Pin {
        Pin {
            image: image.into(),
            digest: Some(digest.into()),
        }
    }

    fn images_of(files: &GeneratedFiles) -> BTreeMap<String, String> {
        let compose: serde_norway::Value =
            serde_norway::from_str(&files[COMPOSE_FILENAME]).unwrap();
        compose["services"]
            .as_mapping()
            .unwrap()
            .iter()
            .filter_map(|(name, entry)| {
                Some((
                    name.as_str()?.to_string(),
                    entry["image"].as_str()?.to_string(),
                ))
            })
            .collect()
    }

    #[test]
    fn a_pinned_service_is_written_with_its_build_and_the_rest_as_generated() {
        let config = config();
        let follows = config.rekuest.image.clone().unwrap();
        let mut files = crate::generate::generate_hub_files(&config, &Default::default());
        let generated = images_of(&files);

        let pins = BTreeMap::from([("rekuest".to_string(), pin(&follows, "sha256:abc"))]);
        apply(&mut files, &config, &pins);

        let written = images_of(&files);
        assert_eq!(written["rekuest"], format!("{follows}@sha256:abc"));
        assert_eq!(written["mikro"], generated["mikro"]);
        assert_eq!(written.len(), generated.len());
        assert_eq!(
            unpinned(&config, &pins).len(),
            config.stack_images().len() - 1
        );
    }

    /// A pin is a channel's: once the profile follows something else, it says nothing.
    #[test]
    fn a_pin_of_another_channel_does_not_count() {
        let mut config = config();
        let pins = BTreeMap::from([(
            "rekuest".to_string(),
            pin(&config.rekuest.image.clone().unwrap(), "sha256:abc"),
        )]);
        assert_eq!(references(&config, &pins).len(), 1);

        config.rekuest.image = Some("jhnnsrs/rekuest:99".into());
        assert!(references(&config, &pins).is_empty());

        // A rollback names the build in the profile itself: nothing to add.
        config.rekuest.image = Some("jhnnsrs/rekuest:99@sha256:old".into());
        let pins = BTreeMap::from([(
            "rekuest".to_string(),
            pin("jhnnsrs/rekuest:99@sha256:old", "sha256:abc"),
        )]);
        assert!(references(&config, &pins).is_empty());
        assert!(!unpinned(&config, &pins)
            .iter()
            .any(|(service, _)| service == "rekuest"));
    }

    #[test]
    fn a_digest_is_taken_from_the_images_own_repository() {
        let digests = vec![
            "mirror.lab/rekuest@sha256:mirror".to_string(),
            "jhnnsrs/rekuest@sha256:hub".to_string(),
        ];
        assert_eq!(
            digest_for("jhnnsrs/rekuest:6", &digests).as_deref(),
            Some("sha256:hub")
        );
        assert_eq!(digest_for("jhnnsrs/mikro:5", &digests), None);
        assert_eq!(digest_for("next-rekuest", &[]), None);
    }

    /// The compose file is people's to edit: pinning it touches its `image:` lines only.
    #[test]
    fn builds_are_written_into_a_compose_file_line_by_line() {
        let compose = "# mine\nservices:\n  rekuest:\n    image: jhnnsrs/rekuest:6\n    command: bash run.sh # kept\n  reaper:\n    image: \"jhnnsrs/rekuest:6\"\n  mikro:\n    image: jhnnsrs/mikro:5\n";
        let channels = BTreeMap::from([(
            "jhnnsrs/rekuest:6".to_string(),
            "jhnnsrs/rekuest:6@sha256:abc".to_string(),
        )]);
        assert_eq!(
            write_into(compose, &channels),
            "# mine\nservices:\n  rekuest:\n    image: jhnnsrs/rekuest:6@sha256:abc\n    command: bash run.sh # kept\n  reaper:\n    image: jhnnsrs/rekuest:6@sha256:abc\n  mikro:\n    image: jhnnsrs/mikro:5\n"
        );
        // Already written: nothing to do a second time.
        assert_eq!(
            write_into(&write_into(compose, &channels), &channels),
            write_into(compose, &channels)
        );
    }
}
