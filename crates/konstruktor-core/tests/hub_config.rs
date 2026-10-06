use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use konstruktor_core::catalog::ServiceId;
use konstruktor_core::config::hub::{
    build_hub_config, storage_mode_of, HubConfig, HubConfigOptions, ServiceOptions, StorageMode,
};
use serde_norway::Value;

mod support;

/// The oracle is the profile fixture the golden tests generate from.
///
/// Values cannot be compared — every secret is freshly generated — so this checks the
/// *shape*: the same keys, at every level. A profile is read back by every later command,
/// and a key that is written under another name than it is read by is a hub that cannot
/// be opened again — a long way from where the bug was written.

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture_config(name: &str) -> Value {
    let text = std::fs::read_to_string(fixtures().join(name)).expect("fixture is readable");
    let profile: Value = serde_norway::from_str(&text).expect("fixture parses");
    profile["config"].clone()
}

fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_mapping()
        .expect("a mapping")
        .keys()
        .filter_map(|k| k.as_str().map(str::to_string))
        .collect()
}

/// A new hub, with every service provided what its image asks for — what `hub create`
/// writes once the images have answered.
fn built() -> HubConfig {
    support::hub(&HubConfigOptions {
        device_id: "device".into(),
        coord_server: "go.arkitekt.live".into(),
        ..Default::default()
    })
}

fn as_value(config: &HubConfig) -> Value {
    serde_norway::to_value(config).expect("the config serializes")
}

#[test]
fn produces_the_same_top_level_shape_as_the_fixture() {
    let ours = as_value(&built());
    let theirs = fixture_config("hub_config.yaml");
    assert_eq!(keys(&ours), keys(&theirs));
}

/// The services are one map, by name, and every service of the catalogue is in it —
/// switched on or not.
#[test]
fn the_services_are_a_map_with_every_catalogue_service_in_it() {
    let ours = as_value(&built());
    let names = keys(&ours["services"]);
    let catalogue: BTreeSet<String> = konstruktor_core::catalog::SERVICE_IDS
        .iter()
        .map(|id| id.as_str().to_string())
        .collect();
    assert_eq!(names, catalogue);
    for name in &catalogue {
        assert!(
            ours.get(name.as_str()).is_none(),
            "`{name}` is a top-level key"
        );
    }
}

#[test]
fn produces_the_same_shape_for_every_block() {
    let built = built();
    let ours = as_value(&built);
    let theirs = fixture_config("hub_config.yaml");

    for block in ["gateway", "db", "minio", "local_redis"] {
        assert_eq!(
            keys(&ours[block]),
            keys(&theirs[block]),
            "block `{block}` has a different shape"
        );
    }

    // The fixture holds the services its hub was written with; each is compared as far as
    // both sides have been provided for. Only Rekuest holds a key there (the fixture
    // predates the others'), and Lovekit is switched on without an image, which a hub
    // built today never is.
    let running: Vec<&str> = built
        .enabled_services()
        .into_iter()
        .map(|id| id.as_str())
        .collect();
    for service in keys(&theirs["services"]) {
        // One that is switched off was never asked what it needs, so it holds none of it.
        if !running.contains(&service.as_str()) {
            continue;
        }
        let mut ours = keys(&ours["services"][&service]);
        let mut theirs = keys(&theirs["services"][&service]);
        ours.remove("instance_key_pair");
        theirs.remove("instance_key_pair");
        if service == "lovekit" {
            ours.remove("image");
        }
        assert_eq!(ours, theirs, "service `{service}` has a different shape");
    }
}

/// A purpose is the service's own word: a block holds a bucket for whatever its image
/// declares, in the order it declares them, and reads it back the same.
#[test]
fn a_service_can_store_into_any_purpose() {
    let config = built();
    let mikro: Vec<(&str, &str)> = config.service(ServiceId::Mikro).buckets.iter().collect();
    assert_eq!(
        mikro,
        [
            ("media", "mikromedia"),
            ("zarr", "mikrozarr"),
            ("parquet", "mikroparquet"),
            ("bigfile", "mikrobigfile"),
            ("fabriks", "mikrofabriks"),
            ("konnektion", "mikrokonnektion"),
        ]
    );

    let mut config = config;
    let mut asks_for_more = support::description_of(ServiceId::Mikro);
    asks_for_more.needs.storage.push("point-clouds".into());
    config.provide(&[("mikro".to_string(), asks_for_more)].into());
    let yaml = serde_norway::to_string(&config).expect("serializes");
    let back: HubConfig = serde_norway::from_str(&yaml).expect("reads back");
    assert_eq!(back, config);
    let buckets = &back.service(ServiceId::Mikro).buckets;
    assert_eq!(
        buckets.get("point-clouds").map(|b| b.bucket_name.as_str()),
        Some("mikropoint-clouds")
    );
    // After the ones there were, which keep their place and their names.
    assert_eq!(buckets.names().last().unwrap(), "mikropoint-clouds");
    assert_eq!(buckets.names()[0], "mikromedia");
}

/// Nothing is assumed of a service whose image was not asked: no buckets, no key.
#[test]
fn a_hub_nothing_was_said_about_holds_no_described_facts() {
    let config = build_hub_config(&HubConfigOptions::default());
    for id in config.service_ids() {
        let block = config.service(id);
        assert!(block.buckets.is_empty(), "{id}");
        assert!(block.instance_key_pair.is_none(), "{id}");
        assert!(
            block.secrets.is_empty() && block.identifier.is_none(),
            "{id}"
        );
    }
}

/// Rekuest follows the `rekuest_server` answer, not the service picker: a hub that trusts
/// a remote Rekuest must not start a second one.
#[test]
fn rekuest_follows_the_provenance_answer_not_the_picker() {
    let local = build_hub_config(&HubConfigOptions {
        rekuest_server: "local".into(),
        services: Some(vec![ServiceId::Mikro]),
        ..Default::default()
    });
    assert!(local.rekuest().enabled, "local rekuest must run here");

    let remote = build_hub_config(&HubConfigOptions {
        rekuest_server: "rekuest.example.org".into(),
        // Explicitly ticked, and still overridden.
        services: Some(vec![ServiceId::Rekuest, ServiceId::Mikro]),
        ..Default::default()
    });
    assert!(!remote.rekuest().enabled);
    assert!(remote.service(ServiceId::Mikro).enabled);
}

/// One service can run from source without the rest of the hub becoming a dev hub, and
/// `--dev` still means all of them. Both answers land on the same `mount_github`, which
/// is what the compose bind mounts and the clone loop are driven from.
#[test]
fn source_mode_can_be_asked_for_one_service_at_a_time() {
    let one = build_hub_config(&HubConfigOptions {
        services: Some(vec![ServiceId::Rekuest, ServiceId::Mikro]),
        service_options: BTreeMap::from([(
            ServiceId::Mikro,
            ServiceOptions {
                from_source: true,
                branch: Some("main".into()),
                ..Default::default()
            },
        )]),
        ..Default::default()
    });
    assert!(
        one.service(ServiceId::Mikro).mount_github,
        "the service that asked runs from source"
    );
    assert!(!one.rekuest().mount_github, "and nothing else does");

    let all = build_hub_config(&HubConfigOptions {
        services: Some(vec![ServiceId::Rekuest, ServiceId::Mikro]),
        dev_hub: true,
        ..Default::default()
    });
    assert!(all.rekuest().mount_github && all.service(ServiceId::Mikro).mount_github);
}

/// The default is a named volume for each: upstream's container-absolute `/data` would be
/// an *anonymous* volume, and a bind mount into the folder is the slow path on every
/// desktop engine. An empty `mount` is what the generator reads as "use `volume_name`".
#[test]
fn storage_lives_in_docker_volumes_by_default() {
    let config = built();
    assert_eq!(storage_mode_of(&config), StorageMode::DockerVolumes);
    assert_eq!(config.minio.mount, None);
    assert_eq!(config.db.mount, None);
    assert_eq!(config.db.volume_name, "db_data");
    assert_eq!(config.minio.volume_name, "rustfs_data");
}

/// The opt-out keeps both mounts relative, so the deployment stays one movable folder.
#[test]
fn storage_can_be_kept_inside_the_deployment_folder() {
    let config = build_hub_config(&HubConfigOptions {
        device_id: "device".into(),
        coord_server: "go.arkitekt.live".into(),
        storage: StorageMode::DeploymentFolder,
        ..Default::default()
    });
    assert_eq!(storage_mode_of(&config), StorageMode::DeploymentFolder);
    assert_eq!(config.minio.mount.as_deref(), Some("./rustfs_data"));
    assert_eq!(config.db.mount.as_deref(), Some("./db_data"));
}

/// A key that does not apply is left out rather than written as null, so a block says only
/// what its service has.
#[test]
fn optional_keys_are_absent_rather_than_null() {
    let config = built();
    let yaml = serde_norway::to_string(&config).expect("serializes");

    assert!(
        !yaml.contains("mesh:"),
        "a hub with no mesh must carry no mesh key"
    );
    assert!(!yaml.contains("image: null"));
    assert!(!yaml.contains("identifier: null"));
    assert!(!yaml.contains("ollama_config: null"));
    assert!(!yaml.contains("instance_key_pair: null"));
    assert!(!yaml.contains("ensured_repositories: null"));

    // The keys that are nullable by design still have to be written.
    assert!(yaml.contains("csrf_trusted_origins: null"));
    assert!(yaml.contains("ssl_cert: null"));
}

/// JavaScript's `||` falls through on `""`, not just on null. A direct `unwrap_or` would
/// write an empty admin password instead of generating one.
#[test]
fn an_empty_admin_password_is_generated_not_written_blank() {
    let config = build_hub_config(&HubConfigOptions {
        global_admin_password: Some("   ".into()),
        ..Default::default()
    });
    assert_eq!(config.global_admin_password.len(), 40);

    let given = build_hub_config(&HubConfigOptions {
        global_admin_password: Some("hunter22-and-then-some".into()),
        ..Default::default()
    });
    assert_eq!(given.global_admin_password, "hunter22-and-then-some");
}

#[test]
fn a_skipped_answer_becomes_null_not_an_empty_string() {
    let config = build_hub_config(&HubConfigOptions {
        domain: Some("  ".into()),
        global_description: Some(String::new()),
        ..Default::default()
    });
    assert_eq!(config.domain, None);
    assert_eq!(config.global_description, None);
}

/// Every image the arkitekt project publishes follows `latest`, and so does RustFS: a hub
/// is created on what was released last. The database major moving under a running hub
/// is `updates::guard`'s to catch, not a pin's. Caddy and Redis keep their exact version
/// tags — third-party images whose `latest` nobody here releases.
#[test]
fn the_infrastructure_follows_latest_except_the_third_party_pins() {
    let config = built();
    let tag = |image: &str| konstruktor_core::status::image_tag(image);

    for (service, image) in [
        ("db", config.db.image.clone()),
        ("rustfs", config.minio.image.clone()),
        ("rustfs_init", config.minio.init_container_image.clone()),
    ] {
        assert_eq!(
            tag(&image).as_deref(),
            Some("latest"),
            "{service} is `{image}`"
        );
        assert!(
            !image.contains("@sha256:"),
            "{service} is `{image}`, pinned by digest"
        );
    }

    for (service, image) in [
        ("gateway", config.gateway.image.clone()),
        ("redis", config.local_redis.image.clone()),
    ] {
        let tag = tag(&image).unwrap_or_else(|| panic!("{service} is `{image}`, with no tag"));
        assert!(
            tag.chars().any(|c| c.is_ascii_digit()),
            "{service} is `{image}`, whose tag names no version"
        );
    }
}
