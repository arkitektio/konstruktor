//! What a newer release newly declares, an update and a regeneration provide.
//!
//! A database is in the hub's own Postgres, a bucket in its own object store, a secret a
//! file in its own folder: none needs anybody's leave. So a release that asks for one more
//! of any of them gets it where its description is taken in — the profile gains it, and
//! the files that are written name it. The one thing a hub cannot give itself is a key
//! somebody else has to vouch for: a release that newly asks for one is refused, before
//! anything is written, and sent to be authorized.

use std::path::{Path, PathBuf};

use konstruktor_core::catalog::ServiceId;
use konstruktor_core::config::hub::{
    build_hub_config, HubConfig, HubConfigOptions, LOCAL_COORD_SERVER,
};
use konstruktor_core::contract::{self, Description, Said};
use konstruktor_core::generate::{generate_hub_files, IssuedIdentity};
use konstruktor_core::lock::{self, Frozen};
use konstruktor_core::profile::{self, hub_profile, write_profile, ProfileError};
use konstruktor_core::services::{self, ProvideError};
use serde_norway::Value;

mod support;

fn example() -> ServiceId {
    ServiceId::named("example")
}

fn options() -> HubConfigOptions {
    HubConfigOptions {
        device_id: "device".into(),
        coord_server: "go.arkitekt.live".into(),
        services: Some(vec![ServiceId::Mikro, example()]),
        ..Default::default()
    }
}

/// What the images say, with `example` saying `example_says`.
fn said_with(example_says: Description) -> Said {
    let mut said = support::said();
    said.insert("example".to_string(), example_says);
    said
}

/// A hub of Mikro and `example`, provided what the release of `example` it runs asks for.
fn hub_from(options: &HubConfigOptions) -> HubConfig {
    let mut config = build_hub_config(options);
    config.set_service_image("example", "example:1");
    config.provide(&said_with(support::example()));
    config
}

/// The next release of `example`: one more bucket, one more database, one more secret.
fn next_release() -> Description {
    let mut said = support::example();
    said.needs.storage.push("thumbnails".into());
    said.needs.databases.push("events".into());
    said.needs.secrets.push("webhook".into());
    said
}

/// The release after that: it signs what it sends, and asks for a key to do it with.
fn release_that_signs() -> Description {
    let mut said = support::example();
    said.needs.instance_key = true;
    said
}

/// A hub's folder as `hub create` leaves it: the profile, the generated files, and what
/// the images said written down.
fn folder(config: &HubConfig, said: &Said) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "konstruktor-provide-{}-{}",
        std::process::id(),
        rand::random::<u32>()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    write_profile(&dir, &hub_profile(config.clone())).unwrap();
    let files = generate_hub_files(config, &IssuedIdentity::default(), said);
    konstruktor_core::migrate::write_hub(&dir, config, &files).unwrap();
    contract::remember(&dir, config, said).unwrap();
    dir
}

/// Every file under `dir`, by path, so "nothing was written" can be an equality.
fn snapshot(dir: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).unwrap().flatten() {
            if entry.path().is_dir() {
                pending.push(entry.path());
            } else {
                out.insert(entry.path(), std::fs::read(entry.path()).unwrap());
            }
        }
    }
    out
}

fn yaml(text: &str) -> Value {
    serde_norway::from_str(text).expect("valid YAML")
}

#[test]
fn a_release_that_declares_more_is_provided_it() {
    let mut config = hub_from(&options());
    let before = config.clone();
    let said = said_with(next_release());

    let provided = services::provide_declared(&mut config, &said).expect("all its own to give");
    assert_eq!(provided.databases, ["example_events"]);
    assert_eq!(provided.buckets, ["examplethumbnails"]);
    assert_eq!(provided.secrets, ["secrets/example.webhook"]);

    // In the block, beside what it held, which is what it was.
    let block = config.service(example());
    let was = before.service(example());
    assert_eq!(block.database("events"), Some("example_events"));
    assert_eq!(block.database("main"), was.database("main"));
    assert_eq!(
        block.buckets.names(),
        ["examplearchive", "examplethumbnails"]
    );
    assert!(block.secrets.contains_key("webhook"));
    assert_eq!(block.secrets.get("signing"), was.secrets.get("signing"));
    // Nobody else's block moved.
    assert_eq!(
        config.service(ServiceId::Mikro),
        before.service(ServiceId::Mikro)
    );

    // In what is generated: the init list, the bucket manifest, the gateway, the mounts.
    let files = generate_hub_files(&config, &IssuedIdentity::default(), &said);
    let compose = yaml(&files["docker-compose.yaml"]);
    assert_eq!(
        compose["services"]["db"]["environment"]["POSTGRES_MULTIPLE_DATABASES"].as_str(),
        Some("rekuest_main,mikro_main,example_events,example_main")
    );
    let buckets: Vec<String> = yaml(&files["configs/rustfs_init.yaml"])["buckets"]
        .as_sequence()
        .unwrap()
        .iter()
        .filter_map(|bucket| bucket["name"].as_str().map(str::to_string))
        .collect();
    assert!(
        buckets.contains(&"examplethumbnails".to_string()),
        "{buckets:?}"
    );
    assert!(files["configs/Caddyfile"].contains("@examplethumbnails path /examplethumbnails*"));
    assert!(files.contains_key("secrets/example.webhook"));
    let mounts = compose["services"]["example"]["volumes"]
        .as_sequence()
        .unwrap();
    assert!(mounts.contains(&Value::from(
        "./secrets/example.webhook:/secrets/example.webhook:ro"
    )));

    // And in what the service is told.
    let facts = contract::facts(&config, example(), &IssuedIdentity::default(), &said);
    assert_eq!(
        facts["databases"]["events"]["name"].as_str(),
        Some("example_events")
    );
    assert_eq!(
        facts["storage"]["buckets"]["thumbnails"].as_str(),
        Some("examplethumbnails")
    );
    assert_eq!(
        facts["secrets"]["webhook"].as_str(),
        Some("/secrets/example.webhook")
    );

    // Asked again, there is nothing more to provide.
    let again = services::provide_declared(&mut config, &said).unwrap();
    assert!(again.is_empty());
}

/// A key is the one thing somebody else has to vouch for. A release that newly asks for
/// one is refused with the profile as it was, and told what it takes.
#[test]
fn a_release_that_newly_asks_for_a_key_is_refused() {
    let mut config = hub_from(&options());
    let before = config.clone();
    let said = said_with(release_that_signs());

    assert_eq!(services::awaiting_key(&config, &said), ["example"]);
    let refused = services::provide_declared(&mut config, &said).unwrap_err();
    assert_eq!(
        refused,
        ProvideError::NeedsAuthorization {
            service: "example".into()
        }
    );
    let why = refused.to_string();
    assert!(why.contains("`example`"), "{why}");
    assert!(why.contains("konstruktor authorize"), "{why}");
    assert_eq!(config, before, "nothing was taken in");

    // Even when the same release also asks for what could have been given.
    let mut both = next_release();
    both.needs.instance_key = true;
    assert!(services::provide_declared(&mut config, &said_with(both)).is_err());
    assert_eq!(config, before);

    // A service that holds a key already is not waiting for one.
    assert!(services::awaiting_key(&config, &support::said()).is_empty());
}

/// A hub that runs its own coordination server writes its trust bundle itself: there a
/// key is trusted the moment the files are written, and nobody has to be asked.
#[test]
fn a_self_contained_hub_mints_the_key_itself() {
    let mut config = hub_from(&HubConfigOptions {
        coord_server: LOCAL_COORD_SERVER.into(),
        ..options()
    });
    let said = said_with(release_that_signs());
    assert!(services::awaiting_key(&config, &said).is_empty());
    services::provide_declared(&mut config, &said).expect("its own to give here");
    assert!(config.service(example()).instance_key_pair.is_some());
}

/// A database that cannot be given is refused the same way, with nothing taken in.
#[test]
fn a_release_that_asks_for_what_cannot_be_given_is_refused() {
    let mut config = hub_from(&options());
    let before = config.clone();
    let mut bad = support::example();
    bad.needs.databases.push("Events".into());
    let refused = services::provide_declared(&mut config, &said_with(bad)).unwrap_err();
    assert!(
        matches!(&refused, ProvideError::Unacceptable(why) if why.contains("`Events`")),
        "{refused}"
    );
    assert_eq!(config, before);
}

/// `hub regenerate` on a hub whose image has since said it needs more: the profile gains
/// it and every file that names it is written.
#[test]
fn regenerating_provides_what_the_images_have_said() {
    let config = hub_from(&options());
    let dir = folder(&config, &said_with(support::example()));
    // The release it runs now says more than the block holds.
    contract::remember(&dir, &config, &said_with(next_release())).unwrap();

    let provided = profile::regenerate(&dir).expect("regenerates");
    assert_eq!(provided.databases, ["example_events"]);
    assert_eq!(provided.buckets, ["examplethumbnails"]);
    assert_eq!(provided.secrets, ["secrets/example.webhook"]);

    let written = profile::read_profile(&dir).unwrap().config;
    let block = written.service(example());
    assert_eq!(block.database("events"), Some("example_events"));
    assert!(block.buckets.get("thumbnails").is_some());
    // What it held is untouched: the same secret, the same bucket.
    assert_eq!(
        block.secrets.get("signing"),
        config.service(example()).secrets.get("signing")
    );
    let secret = std::fs::read_to_string(dir.join("secrets/example.webhook")).unwrap();
    assert_eq!(secret.trim(), block.secrets["webhook"]);
    let compose = yaml(&std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap());
    assert!(
        compose["services"]["db"]["environment"]["POSTGRES_MULTIPLE_DATABASES"]
            .as_str()
            .unwrap()
            .contains("example_events")
    );
    assert!(
        std::fs::read_to_string(dir.join("configs/rustfs_init.yaml"))
            .unwrap()
            .contains("examplethumbnails")
    );

    // A second time there is nothing left to provide, and nothing changes.
    let settled = snapshot(&dir);
    assert!(profile::regenerate(&dir).unwrap().is_empty());
    let mut again = snapshot(&dir);
    // The compose file's backup is the one file a regeneration always writes.
    let backup = dir.join(konstruktor_core::compose_file::COMPOSE_BACKUP_FILENAME);
    again.remove(&backup);
    let mut settled = settled;
    settled.remove(&backup);
    assert_eq!(again, settled);
    std::fs::remove_dir_all(&dir).ok();
}

/// And one that newly asks for a key is refused there too: not a file is written, the
/// compose file's backup included.
#[test]
fn regenerating_writes_nothing_for_a_release_that_asks_for_a_key() {
    let config = hub_from(&options());
    let dir = folder(&config, &said_with(support::example()));
    contract::remember(&dir, &config, &said_with(release_that_signs())).unwrap();
    let before = snapshot(&dir);

    let refused = profile::regenerate(&dir).unwrap_err();
    assert!(
        matches!(
            &refused,
            ProfileError::Provide(ProvideError::NeedsAuthorization { service }) if service == "example"
        ),
        "{refused}"
    );
    let why = refused.to_string();
    assert!(why.contains("konstruktor authorize"), "{why}");
    assert!(why.contains("Nothing was written"), "{why}");
    assert_eq!(snapshot(&dir), before, "the folder is as it was");
    std::fs::remove_dir_all(&dir).ok();
}

/// A frozen hub whose files are behind is not moved in passing: the move would take every
/// service along, which is what the freeze forbids. It is refused before anything is
/// backed up, copied or written.
#[tokio::test]
async fn a_frozen_hub_that_is_behind_is_not_updated() {
    use konstruktor_core::updates::{self, UpdateError, UpdateRequest};

    let config = support::hub(&HubConfigOptions {
        device_id: "device".into(),
        coord_server: "go.arkitekt.live".into(),
        ..Default::default()
    });
    let dir = folder(&config, &support::said());
    // Its files are of the layout before this build's, as a hub an earlier Konstruktor
    // of this profile's time generated would be — and one of its services is held.
    let mut held = lock::read(&dir);
    held.layout = Some(konstruktor_core::migrate::CURRENT_LAYOUT - 1);
    held.frozen.insert("mikro".into(), Frozen { at: 1 });
    lock::write(&dir, &held).unwrap();
    assert!(!konstruktor_core::migrate::pending(&dir, &config).is_empty());
    let before = snapshot(&dir);

    let refused = updates::apply(
        &dir,
        &UpdateRequest {
            services: vec!["rekuest".into()],
            advances: Vec::new(),
            pull: true,
            backup_into: Some(dir.join("backups")),
            health_check: true,
        },
        &|_| {},
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&refused, UpdateError::Frozen(why) if why.contains("konstruktor unfreeze")),
        "{refused}"
    );
    assert_eq!(
        snapshot(&dir),
        before,
        "nothing was backed up, copied or written"
    );
    assert!(!dir.join("backups").exists() && !dir.join(".konstruktor").exists());

    // The same hub, not behind, is not refused for being frozen: only the held service is.
    held.layout = Some(konstruktor_core::migrate::CURRENT_LAYOUT);
    lock::write(&dir, &held).unwrap();
    assert!(konstruktor_core::migrate::pending(&dir, &config).is_empty());
    std::fs::remove_dir_all(&dir).ok();
}
