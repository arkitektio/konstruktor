use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use konstruktor_core::catalog::ServiceId;
use konstruktor_core::config::hub::{
    build_hub_config, storage_mode_of, HubConfig, HubConfigOptions, ServiceOptions, StorageMode,
};
use serde_norway::Value;

/// The oracle is a profile the real Python CLI wrote.
///
/// Values cannot be compared — every secret is freshly generated — so this checks the
/// *shape*: the same keys, at every level. A missing key would surface downstream as a
/// pydantic error the moment the CLI next read the folder, which is a long way from where
/// the bug was written.

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

fn built() -> HubConfig {
    build_hub_config(&HubConfigOptions {
        device_id: "device".into(),
        coord_server: "go.arkitekt.live".into(),
        ..Default::default()
    })
}

fn as_value(config: &HubConfig) -> Value {
    serde_norway::to_value(config).expect("the config serializes")
}

#[test]
fn produces_the_same_top_level_shape_as_the_python_cli() {
    let ours = as_value(&built());
    let theirs = fixture_config("hub_config.yaml");
    assert_eq!(keys(&ours), keys(&theirs));
}

#[test]
fn produces_the_same_shape_for_every_block() {
    let ours = as_value(&built());
    let theirs = fixture_config("hub_config.yaml");

    let mut blocks: Vec<String> = vec![
        "gateway".into(),
        "db".into(),
        "minio".into(),
        "local_redis".into(),
    ];
    blocks.extend(
        konstruktor_core::catalog::SERVICE_IDS
            .iter()
            .map(|id| id.as_str().to_string()),
    );

    for block in blocks {
        assert_eq!(
            keys(&ours[&block]),
            keys(&theirs[&block]),
            "block `{block}` has a different shape"
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
    assert!(local.rekuest.enabled, "local rekuest must run here");

    let remote = build_hub_config(&HubConfigOptions {
        rekuest_server: "rekuest.example.org".into(),
        // Explicitly ticked, and still overridden.
        services: Some(vec![ServiceId::Rekuest, ServiceId::Mikro]),
        ..Default::default()
    });
    assert!(!remote.rekuest.enabled);
    assert!(remote.mikro.enabled);
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
        one.mikro.mount_github,
        "the service that asked runs from source"
    );
    assert!(!one.rekuest.mount_github, "and nothing else does");

    let all = build_hub_config(&HubConfigOptions {
        services: Some(vec![ServiceId::Rekuest, ServiceId::Mikro]),
        dev_hub: true,
        ..Default::default()
    });
    assert!(all.rekuest.mount_github && all.mikro.mount_github);
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
    assert_eq!(config.minio.volume_name, "minio_data");
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
    assert_eq!(config.minio.mount.as_deref(), Some("./minio_data"));
    assert_eq!(config.db.mount.as_deref(), Some("./db_data"));
}

/// `extra="forbid"` upstream: a key present-but-null is a hard failure where an absent
/// key is fine. This is the easiest way to break a generated profile, so assert it.
#[test]
fn optional_keys_are_absent_rather_than_null() {
    let config = built();
    let yaml = serde_norway::to_string(&config).expect("serializes");

    assert!(
        !yaml.contains("mesh:"),
        "a hub with no mesh must carry no mesh key"
    );
    // Lovekit declares no image, and every service but mikro/elektro declares no zarr.
    assert!(!yaml.contains("image: null"));
    assert!(!yaml.contains("zarr_bucket: null"));
    assert!(!yaml.contains("ollama_config: null"));
    assert!(!yaml.contains("provenance_key_pair: null"));
    assert!(!yaml.contains("ensured_repositories: null"));

    // The keys that *are* nullable upstream still have to be written.
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

/// The infrastructure a new hub gets must not float on a mutable tag.
///
/// The services follow a channel on purpose — that is what a channel is here. The four
/// underneath them do not: a `db` image that silently became the next Postgres major
/// leaves a cluster the new binary refuses to open, and no amount of rolling the image
/// back undoes what a migration did on the way. Immutable means one of two things, both
/// allowed: a digest, for `jhnnsrs/daten` and `jhnnsrs/init`, which publish only channel
/// tags and so have no version to name; or an exact version tag, for the images that
/// publish one.
#[test]
fn the_infrastructure_is_pinned_to_something_immutable() {
    let config = built();
    let infrastructure = [
        ("db", config.db.image.clone()),
        ("gateway", config.gateway.image.clone()),
        ("redis", config.local_redis.image.clone()),
        ("minio", config.minio.image.clone()),
        ("minio_init", config.minio.init_container_image.clone()),
    ];

    for (service, image) in infrastructure {
        if image.contains("@sha256:") {
            continue;
        }
        let tag = konstruktor_core::status::image_tag(&image)
            .unwrap_or_else(|| panic!("{service} is `{image}` — no tag and no digest at all"));
        assert!(
            !["latest", "dev", "next", "prod", "prodx", "release", "nightly"]
                .contains(&tag.as_str()),
            "{service} is pinned to `{image}`, and `{tag}` is a channel that moves — \
             pin it to a version or a digest"
        );
        assert!(
            tag.chars().any(|c| c.is_ascii_digit()),
            "{service} is pinned to `{image}`, whose tag names no version"
        );
    }
}

/// A digest pin is only a pin if the tag beside it is still readable, because that tag is
/// what `status` reports as the hub's channel.
#[test]
fn a_digest_pinned_image_still_says_which_channel_it_came_from() {
    let config = built();
    assert_eq!(
        konstruktor_core::status::image_tag(&config.db.image).as_deref(),
        Some("dev"),
        "the database pin dropped its tag, so `status` can no longer say what it follows"
    );
}
