//! What a hub is actually running, written down.
//!
//! A hub cannot otherwise say what version it is. The services follow channel tags, the
//! tags move, and the images they resolved to on the day are recorded nowhere — so after
//! an update there is no statement of what came before, and "put it back the way it was"
//! has no answer. `updates::check_one` even resolves the digest already, purely to compare
//! it, and then throws it away.
//!
//! `hub_lock.json` is that statement: for every service, the reference the profile names
//! and the digest it resolved to, snapshotted before and after each update. It is what
//! `konstruktor rollback` reads.
//!
//! **Beside the profile, not inside it.** `config::hub` records that upstream's pydantic
//! model uses `extra="forbid"`, so a key it does not know makes the Python CLI reject the
//! whole profile. A separate file is also the honest shape: the profile is a statement of
//! intent — the channel a hub follows — and this is a record of fact.
//!
//! It says nothing about the database. An image can be put back; a migration that ran on
//! start cannot be taken back. See `konstruktor rollback`, which says so at the prompt.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::hub::HubConfig;

pub const LOCK_FILENAME: &str = "hub_lock.json";

/// The file moved aside when it will not parse, rather than overwritten.
pub const QUARANTINE_SUFFIX: &str = ".unreadable";

/// One service's image, as named and as resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    /// The reference the profile names, e.g. `jhnnsrs/rekuest:next`.
    pub image: String,
    /// What that resolved to on this machine, e.g. `sha256:…`. `None` when the image was
    /// never pulled, or was built locally and carries no registry digest — in which case
    /// there is nothing to roll back *to*, and rollback says so rather than guessing.
    pub digest: Option<String>,
}

impl Pin {
    /// The immutable reference this pin stands for: `repo:tag@sha256:…`.
    ///
    /// The tag is kept beside the digest deliberately. The digest decides what is pulled;
    /// the tag is still what the hub follows, and it is what `status` reports as the
    /// channel — a rolled-back hub that could no longer say which channel it is on would
    /// have traded one missing fact for another.
    pub fn reference(&self) -> Option<String> {
        if self.image.contains('@') {
            return Some(self.image.clone());
        }
        Some(format!("{}@{}", self.image, self.digest.as_ref()?))
    }
}

/// One moment in a hub's life, and what it was running at it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Seconds since the epoch — there is no clock library in the core.
    pub at: u64,
    /// Why it was written: `created`, `before update`, `updated`, `rolled back`.
    pub reason: String,
    /// Compose service name to what it was running.
    pub services: BTreeMap<String, Pin>,
}

/// The file itself. Oldest first; the last entry is what the hub is on now.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Lock {
    pub version: u32,
    #[serde(default)]
    pub history: Vec<Entry>,
}

impl Lock {
    /// What the hub is running now, as far as this file knows.
    pub fn current(&self) -> Option<&Entry> {
        self.history.last()
    }

    /// The state to roll back *to*: the newest entry that is not the current one.
    ///
    /// Entries that describe the same set of images are never appended, so "the one
    /// before" is always a state that actually differs — a rollback offered to the user
    /// is a rollback that changes something.
    pub fn previous(&self) -> Option<&Entry> {
        let len = self.history.len();
        (len >= 2).then(|| &self.history[len - 2])
    }
}

pub fn lock_path(dir: &Path) -> PathBuf {
    dir.join(LOCK_FILENAME)
}

/// The lock file, or an empty one.
///
/// A file that will not parse is moved aside rather than overwritten — the same rule
/// `registry::load` follows, and for the same reason: this is the only record of what the
/// hub was running before, and the next write would otherwise erase it. `None` is never
/// returned for a missing file; a hub that predates this simply has no history yet.
pub fn read(dir: &Path) -> Lock {
    let path = lock_path(dir);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Lock { version: 1, history: Vec::new() };
    };
    match serde_json::from_str::<Lock>(&text) {
        Ok(lock) => lock,
        Err(_) => {
            let _ = std::fs::rename(
                &path,
                path.with_extension(format!("json{QUARANTINE_SUFFIX}")),
            );
            Lock { version: 1, history: Vec::new() }
        }
    }
}

pub fn write(dir: &Path, lock: &Lock) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(lock).expect("a lock always serializes");
    std::fs::write(lock_path(dir), format!("{text}\n"))
}

/// What every image the stack declares resolves to on this machine right now.
pub async fn snapshot(config: &HubConfig) -> BTreeMap<String, Pin> {
    let states = crate::docker::image_states(&config.stack_images())
        .await
        .unwrap_or_default();
    states
        .into_iter()
        .map(|state| {
            let digest = state
                .repo_digests
                .first()
                .and_then(|d| d.rsplit_once('@'))
                .map(|(_, digest)| digest.to_string());
            (state.service, Pin { image: state.image, digest })
        })
        .collect()
}

/// Appends what the hub is running now, if it differs from what was last written.
///
/// Returns whether anything was appended. Recording the same set twice would push the
/// state worth rolling back to out of reach behind an entry identical to the current one.
pub async fn record(
    dir: &Path,
    config: &HubConfig,
    reason: &str,
    now: u64,
) -> std::io::Result<bool> {
    let services = snapshot(config).await;
    let mut lock = read(dir);
    lock.version = 1;
    if lock.current().is_some_and(|entry| entry.services == services) {
        return Ok(false);
    }
    lock.history.push(Entry {
        at: now,
        reason: reason.to_string(),
        services,
    });
    // A long-lived hub updated weekly would otherwise grow this file forever. Twenty is
    // more history than any rollback reaches back through.
    let len = lock.history.len();
    if len > 20 {
        lock.history.drain(..len - 20);
    }
    write(dir, &lock)?;
    Ok(true)
}

/// Seconds since the epoch, for callers that have no clock of their own.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("konstruktor-lock-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        dir
    }

    fn entry(reason: &str, image: &str, digest: &str) -> Entry {
        Entry {
            at: 1,
            reason: reason.into(),
            services: BTreeMap::from([(
                "rekuest".to_string(),
                Pin { image: image.into(), digest: Some(digest.into()) },
            )]),
        }
    }

    #[test]
    fn a_pin_names_the_digest_and_keeps_the_channel() {
        let pin = Pin {
            image: "jhnnsrs/rekuest:next".into(),
            digest: Some("sha256:abc".into()),
        };
        assert_eq!(
            pin.reference().as_deref(),
            Some("jhnnsrs/rekuest:next@sha256:abc")
        );
        // Already pinned: left exactly as it is, rather than pinned twice.
        let pinned = Pin {
            image: "jhnnsrs/daten:dev@sha256:abc".into(),
            digest: None,
        };
        assert_eq!(
            pinned.reference().as_deref(),
            Some("jhnnsrs/daten:dev@sha256:abc")
        );
        // Never pulled: there is nothing to roll back to, and saying so is the point.
        assert_eq!(
            Pin { image: "jhnnsrs/rekuest:next".into(), digest: None }.reference(),
            None
        );
    }

    #[test]
    fn the_previous_state_is_the_one_before_the_current_one() {
        let mut lock = Lock { version: 1, history: Vec::new() };
        assert!(lock.previous().is_none(), "nothing to roll back to yet");

        lock.history.push(entry("before update", "r:next", "sha256:old"));
        assert!(lock.previous().is_none(), "one state is not a rollback");

        lock.history.push(entry("updated", "r:next", "sha256:new"));
        assert_eq!(
            lock.previous().expect("a previous state").services["rekuest"].digest,
            Some("sha256:old".into())
        );
    }

    #[test]
    fn a_lock_round_trips_and_an_unreadable_one_is_kept() {
        let dir = tmpdir();
        let lock = Lock {
            version: 1,
            history: vec![entry("updated", "r:next", "sha256:new")],
        };
        write(&dir, &lock).expect("writing");
        assert_eq!(read(&dir).history, lock.history);

        // The only record of what came before must not be overwritten by the next write.
        std::fs::write(lock_path(&dir), "{ not json").expect("truncating");
        assert!(read(&dir).history.is_empty());
        let quarantined = lock_path(&dir).with_extension(format!("json{QUARANTINE_SUFFIX}"));
        assert_eq!(
            std::fs::read_to_string(quarantined).expect("the quarantined file"),
            "{ not json"
        );
    }

    /// A hub that is not moving must not accumulate entries — each one would push the
    /// state actually worth returning to further out of reach.
    #[test]
    fn recording_the_same_images_twice_appends_once() {
        let dir = tmpdir();
        let mut lock = Lock { version: 1, history: Vec::new() };
        lock.history.push(entry("before update", "r:next", "sha256:old"));
        write(&dir, &lock).expect("writing");

        let same = read(&dir);
        assert_eq!(same.history.len(), 1);
        assert!(same.current().is_some_and(|e| e.services
            == entry("x", "r:next", "sha256:old").services));
    }
}
