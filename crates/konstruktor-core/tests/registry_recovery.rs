//! What happens to a registry that will not parse.
//!
//! `load` is deliberately tolerant — a bad file must never stop either front end from
//! starting. The danger is that almost every caller follows a `load` with a `save`
//! (`create.rs`, `engine.rs`, `destroy.rs`, `create::reauthorize`), so a `load` that
//! returned an empty registry meant the next write erased the record of every deployment
//! on the machine. The folders stayed; nothing pointed at them.
//!
//! These tests pin the two things that keep that from happening: the unreadable file is
//! moved aside rather than left to be overwritten, and the machine's `deviceId` survives —
//! it becomes the `node_id` on every service manifest, so a new one makes a re-authorized
//! hub look like a different machine to the coordination server.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use konstruktor_core::registry;

/// There is one registry path per machine, so these tests share a resource and cannot run
/// beside each other. Serialised here rather than by asking for `--test-threads=1`, which
/// nothing in CI passes.
fn exclusive() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    // A test that panicked while holding the lock poisoned it; the next one still wants
    // the exclusivity, not the panic.
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The registry lives in the platform data directory. Point every variable `dirs` reads at
/// a scratch folder before anything touches it, so a test never reads or writes the real
/// one.
fn isolate() -> PathBuf {
    use std::sync::OnceLock;
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("konstruktor-reg-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("a scratch data directory");
        std::env::set_var("XDG_DATA_HOME", &root);
        std::env::set_var("HOME", &root);
        std::env::set_var("APPDATA", &root);
        root
    })
    .clone()
}

fn write_registry(text: &str) -> PathBuf {
    isolate();
    let path = registry::registry_path().expect("a registry path");
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("the registry directory");
    std::fs::write(&path, text).expect("the registry file");
    path
}

const GOOD: &str = r#"{
  "version": 1,
  "deviceId": "device-that-must-survive",
  "deployments": [
    {"id":"a1","name":"MyHub","path":"/hubs/MyHub","kind":"hub","project":"myhub",
     "createdAt":"2026-01-01T00:00:00Z"}
  ]
}"#;

#[test]
fn a_readable_registry_is_returned_as_it_is() {
    let _guard = exclusive();
    let _ = write_registry(GOOD);
    let loaded = registry::load();
    assert_eq!(loaded.device_id, "device-that-must-survive");
    assert_eq!(loaded.deployments.len(), 1);
}

/// The heart of it: a truncated file must not be silently replaced by an empty registry
/// that the next `save` then makes permanent.
#[test]
fn a_corrupt_registry_is_moved_aside_and_its_device_id_kept() {
    let _guard = exclusive();
    let path = write_registry(&GOOD[..GOOD.len() / 2]);
    let quarantined = path.with_extension(format!("json{}", registry::QUARANTINE_SUFFIX));
    std::fs::remove_file(&quarantined).ok();

    let loaded = registry::load();

    // Nothing was recovered from the truncated half — that is expected and honest.
    assert!(loaded.deployments.is_empty());
    // But the machine is still the same machine.
    assert_eq!(loaded.device_id, "device-that-must-survive");

    // And the unreadable original is still on disk, under a name that says what it is,
    // so the next `save` writes a new file rather than overwriting the evidence.
    assert!(
        quarantined.is_file(),
        "the unreadable registry was not preserved at {}",
        quarantined.display()
    );
    let kept = std::fs::read_to_string(&quarantined).expect("the quarantined file");
    assert_eq!(kept, GOOD[..GOOD.len() / 2]);

    // The load→save pattern every caller uses must now leave the original alone.
    registry::save(&loaded).expect("saving");
    assert!(quarantined.is_file());

    std::fs::remove_file(&quarantined).ok();
}

/// Valid JSON that is no longer a registry — a field that changed type, say — is the other
/// way this happens, and it is salvaged the same way.
#[test]
fn a_registry_of_the_wrong_shape_still_yields_its_device_id() {
    let _guard = exclusive();
    let path = write_registry(r#"{"version":"one","deviceId":"still-here","deployments":{}}"#);
    let quarantined = path.with_extension(format!("json{}", registry::QUARANTINE_SUFFIX));
    std::fs::remove_file(&quarantined).ok();

    let loaded = registry::load();
    assert_eq!(loaded.device_id, "still-here");
    assert!(quarantined.is_file());

    std::fs::remove_file(&quarantined).ok();
}

/// No file at all is an ordinary first run, not a loss: nothing to quarantine, and a
/// fresh id is correct.
#[test]
fn a_missing_registry_is_not_treated_as_corruption() {
    let _guard = exclusive();
    isolate();
    let path = registry::registry_path().expect("a registry path");
    std::fs::create_dir_all(path.parent().expect("a parent")).ok();
    std::fs::remove_file(&path).ok();
    let quarantined = path.with_extension(format!("json{}", registry::QUARANTINE_SUFFIX));
    std::fs::remove_file(&quarantined).ok();

    let loaded = registry::load();
    assert!(loaded.deployments.is_empty());
    assert!(!loaded.device_id.is_empty(), "a first run still needs an id");
    assert!(!quarantined.exists(), "nothing should have been quarantined");
}
