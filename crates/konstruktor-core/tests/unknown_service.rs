//! A hub hosts a service this build has never heard of.
//!
//! Nothing in the library lists the services there are: what a service needs from a hub,
//! how it is started and what it is registered as, its image says. So a service outside
//! the catalogue — `example` here, described as its image would describe it — has to come
//! out of the generator as much a service as Mikro does: a block in the profile, a
//! database, a bucket, a route on the gateway, a compose service started with the command
//! it names, and an instance in the manifest under its own identifier.

use konstruktor_core::catalog::{ServiceId, SERVICE_IDS};
use konstruktor_core::config::hub::{build_hub_config, HubConfig, HubConfigOptions};
use konstruktor_core::connect::manifest::{build_hub_request, AdvertisedHost, HubManifestOptions};
use konstruktor_core::contract::Said;
use konstruktor_core::generate::{generate_hub_files, GeneratedFiles, IssuedIdentity};
use konstruktor_core::hosts::HostCategory;
use serde_norway::Value;

mod support;

fn example() -> ServiceId {
    ServiceId::named("example")
}

/// What the images of the hub say: the catalogue's, and the one nobody has heard of.
fn said() -> Said {
    let mut said = support::said();
    said.insert("example".to_string(), support::example());
    said
}

/// The hub `hub create --service-image mikro… --service-image example:1` arrives at: the
/// services the images said they are, the unknown one on the image it was named by, and
/// each provided what it asked for.
fn hub() -> HubConfig {
    let mut config = build_hub_config(&HubConfigOptions {
        device_id: "device".into(),
        coord_server: "go.arkitekt.live".into(),
        services: Some(vec![ServiceId::Mikro, example()]),
        ..Default::default()
    });
    config.set_service_image("example", "registry.example.org/example:1");
    config.provide(&said());
    config
}

fn files() -> GeneratedFiles {
    generate_hub_files(&hub(), &IssuedIdentity::default(), &said())
}

fn yaml(files: &GeneratedFiles, name: &str) -> Value {
    serde_norway::from_str(&files[name]).unwrap_or_else(|e| panic!("{name} parses: {e}"))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_sequence()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// It has a block from the moment it is named, with nothing the catalogue would have
/// brought — no image until it is given one, no repository — and the conventions every
/// service is held to.
#[test]
fn it_gets_a_block_of_its_own() {
    let named = build_hub_config(&HubConfigOptions {
        services: Some(vec![example()]),
        ..Default::default()
    });
    let block = named
        .get(example())
        .expect("a block from the moment it is added");
    assert!(block.enabled && block.image.is_none() && !block.runs());
    assert_eq!(named.imageless_services(), ["example"]);
    assert!(!named.enabled_services().contains(&example()));

    let config = hub();
    let block = config.service(example());
    assert!(block.runs());
    assert_eq!(
        block.image.as_deref(),
        Some("registry.example.org/example:1")
    );
    assert_eq!(block.host, "example");
    assert_eq!(block.internal_port, 80);
    assert_eq!(block.github_repo, None);
    assert_eq!(block.database("main"), Some("example_main"));
    // What its image asked for, and only that.
    assert_eq!(block.identifier.as_deref(), Some("org.example.service"));
    assert_eq!(
        block.buckets.iter().collect::<Vec<_>>(),
        [("archive", "examplearchive")]
    );
    assert_eq!(
        block.secrets.keys().collect::<Vec<_>>(),
        ["signing"],
        "the one secret it declared"
    );
    assert!(
        block.instance_key_pair.is_none(),
        "it asked for no instance key"
    );
    assert_eq!(block.health_path(), "healthz");

    // A hub nobody named it in has no block for it, and says so without panicking.
    let without = build_hub_config(&HubConfigOptions::default());
    assert!(without.get(example()).is_none());
    assert!(!without.runs(example()));
}

/// The catalogue's services first, in the order they always had; the rest after, by name.
#[test]
fn it_is_generated_after_the_catalogues_services() {
    let mut config = build_hub_config(&HubConfigOptions {
        services: Some(vec![
            ServiceId::named("zebra"),
            ServiceId::Mikro,
            example(),
            ServiceId::Kabinet,
        ]),
        ..Default::default()
    });
    config.set_service_image("example", "example:1");
    config.set_service_image("zebra", "zebra:1");
    let order: Vec<&str> = config
        .enabled_services()
        .iter()
        .map(|id| id.as_str())
        .collect();
    assert_eq!(order, ["rekuest", "kabinet", "mikro", "example", "zebra"]);
}

/// The profile holds it under `services`, beside the catalogue's, and reads back as it
/// was written.
#[test]
fn the_profile_holds_it_like_any_service() {
    let config = hub();
    let text = serde_norway::to_string(&config).expect("serializes");
    let value: Value = serde_norway::from_str(&text).expect("parses");
    assert_eq!(
        value["services"]["example"]["image"].as_str(),
        Some("registry.example.org/example:1")
    );
    assert_eq!(
        value["services"]["example"]["buckets"]["archive"]["bucket_name"].as_str(),
        Some("examplearchive")
    );
    assert!(value["services"]["example"]["secrets"]["signing"].is_string());
    let names = value["services"].as_mapping().expect("a map").len();
    assert_eq!(names, SERVICE_IDS.len() + 1);

    let back: HubConfig = serde_norway::from_str(&text).expect("reads back");
    assert_eq!(back, config);
}

#[test]
fn it_gets_a_database_a_bucket_and_a_gateway_route() {
    let files = files();
    let compose = yaml(&files, "docker-compose.yaml");

    let databases = compose["services"]["db"]["environment"]["POSTGRES_MULTIPLE_DATABASES"]
        .as_str()
        .expect("the init list");
    assert_eq!(databases, "rekuest_main,mikro_main,example_main");

    let buckets: Vec<String> = yaml(&files, "configs/rustfs_init.yaml")["buckets"]
        .as_sequence()
        .expect("buckets")
        .iter()
        .filter_map(|bucket| bucket["name"].as_str().map(str::to_string))
        .collect();
    assert!(
        buckets.contains(&"examplearchive".to_string()),
        "{buckets:?}"
    );
    // After the catalogue's, in the order the services are generated.
    assert_eq!(buckets.last().unwrap(), "examplearchive");

    let caddy = &files["configs/Caddyfile"];
    assert!(caddy.contains("\t@example path /example*\n"), "{caddy}");
    assert!(caddy.contains("\t\treverse_proxy example:80\n"), "{caddy}");
    assert!(caddy.contains("\t@examplearchive path /examplearchive*\n"));
    // Its routes come after every catalogue service's.
    assert!(caddy.find("@example path").unwrap() > caddy.find("@mikro path").unwrap());
}

#[test]
fn it_is_started_with_the_command_its_image_names() {
    let files = files();
    let compose = yaml(&files, "docker-compose.yaml");
    let service = &compose["services"]["example"];

    assert_eq!(
        service["image"].as_str(),
        Some("registry.example.org/example:1")
    );
    assert_eq!(strings(&service["command"]), ["example-server", "serve"]);
    let volumes = strings(&service["volumes"]);
    assert!(
        volumes.contains(&"./configs/example.yaml:/workspace/config.yaml".to_string()),
        "{volumes:?}"
    );
    // Its secret, as a file only it is handed.
    assert!(
        volumes.contains(&"./secrets/example.signing:/secrets/example.signing:ro".to_string()),
        "{volumes:?}"
    );
    assert!(files.contains_key("secrets/example.signing"));
    // No source to mount: nothing here knows its repository.
    assert!(volumes.iter().all(|volume| !volume.contains("mounts/")));
    // And nothing that runs beside one service in particular runs beside it.
    assert!(compose["services"].get("example-takt").is_none());

    let mut debugging = hub();
    debugging.service_mut(example()).debug = true;
    let compose = yaml(
        &generate_hub_files(&debugging, &IssuedIdentity::default(), &said()),
        "docker-compose.yaml",
    );
    assert_eq!(
        strings(&compose["services"]["example"]["command"]),
        ["example-server", "debug"]
    );
}

/// What it is told of the hub is what any service is told: only the vocabulary of the
/// facts, with its own buckets, its own secret and — having asked for none — no key.
#[test]
fn it_is_told_of_the_hub_what_it_asked_for() {
    let config = hub();
    let facts =
        konstruktor_core::contract::facts(&config, example(), &IssuedIdentity::default(), &said());
    assert_eq!(facts["me"]["name"].as_str(), Some("example"));
    assert_eq!(
        facts["me"]["identifier"].as_str(),
        Some("org.example.service")
    );
    assert_eq!(
        facts["databases"]["main"]["name"].as_str(),
        Some("example_main")
    );
    assert_eq!(
        facts["storage"]["buckets"]["archive"].as_str(),
        Some("examplearchive")
    );
    assert_eq!(
        facts["secrets"]["signing"].as_str(),
        Some("/secrets/example.signing")
    );
    assert!(facts.get("instance").is_none());
    // The other services know where it is, and what it is registered as.
    let mikro = konstruktor_core::contract::facts(
        &config,
        ServiceId::Mikro,
        &IssuedIdentity::default(),
        &said(),
    );
    assert_eq!(
        mikro["peers"]["example"]["url"].as_str(),
        Some("http://example:80/example")
    );
    assert_eq!(
        mikro["peers"]["example"]["identifier"].as_str(),
        Some("org.example.service")
    );
}

/// A service that declares it uses neither the database, the Redis nor the operator
/// account is wired to none of them.
#[test]
fn it_is_wired_only_to_what_it_declares() {
    let mut lean = support::example();
    lean.needs.databases.clear();
    lean.needs.redis = false;
    lean.needs.admin = false;
    lean.needs.storage.clear();
    let mut said = support::said();
    said.insert("example".to_string(), lean);

    let mut config = build_hub_config(&HubConfigOptions {
        services: Some(vec![ServiceId::Mikro, example()]),
        ..Default::default()
    });
    config.set_service_image("example", "example:1");
    config.provide(&said);

    let files = generate_hub_files(&config, &IssuedIdentity::default(), &said);
    let compose = yaml(&files, "docker-compose.yaml");
    assert_eq!(
        compose["services"]["db"]["environment"]["POSTGRES_MULTIPLE_DATABASES"].as_str(),
        Some("rekuest_main,mikro_main")
    );
    let waits_on = |service: &str| -> Vec<(String, String)> {
        compose["services"][service]["depends_on"]
            .as_mapping()
            .expect("a map of conditions")
            .iter()
            .map(|(on, how)| {
                (
                    on.as_str().unwrap().to_string(),
                    how["condition"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    };
    assert!(waits_on("example").is_empty());
    let pairs = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(on, how)| (on.to_string(), how.to_string()))
            .collect()
    };
    assert_eq!(
        waits_on("mikro"),
        pairs(&[
            ("db", "service_healthy"),
            ("redis", "service_started"),
            ("rustfs", "service_started"),
            ("rustfs_init", "service_completed_successfully"),
        ])
    );
    // What everything waits on to be healthy has a way of saying that it is.
    assert_eq!(
        strings(&compose["services"]["db"]["healthcheck"]["test"])[..2],
        ["CMD", "pg_isready"]
    );

    let facts =
        konstruktor_core::contract::facts(&config, example(), &IssuedIdentity::default(), &said);
    for absent in ["redis", "storage"] {
        assert!(facts.get(absent).is_none(), "{absent}");
    }
    assert!(facts["databases"]
        .as_mapping()
        .is_some_and(|databases| databases.is_empty()));
    assert!(facts["me"].get("admin").is_none());
    assert!(facts["me"]["secret_key"].is_string());
}

#[test]
fn the_manifest_registers_it_as_its_image_describes_it() {
    let config = hub();
    let request = build_hub_request(
        &config,
        &HubManifestOptions {
            identifier: "lab-hub".into(),
            hosts: vec![AdvertisedHost {
                host: "10.0.0.4".into(),
                kind: HostCategory::Private,
            }],
            described: said(),
            ..Default::default()
        },
    );
    let instance = request
        .hub
        .instances
        .iter()
        .find(|instance| instance.manifest.identifier == "org.example.service")
        .expect("it is registered under the identifier its image gives");

    // Display text from the description: the catalogue has no word for it.
    assert_eq!(instance.identifier, "example");
    assert_eq!(
        instance.description.as_deref(),
        Some("A minimal service, to read and to copy.")
    );
    let keys = |entries: &[konstruktor_core::connect::manifest::ManifestEntry]| {
        entries
            .iter()
            .map(|entry| entry.key.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(keys(&instance.manifest.scopes), ["example_read"]);
    assert_eq!(keys(&instance.manifest.roles), ["curator"]);
    assert!(instance.manifest.public_sources.is_empty());
    assert!(
        instance.manifest.challenge_key.is_none(),
        "it holds no instance key to send"
    );
    // Reached under its own path, and checked where it says it answers.
    assert_eq!(instance.aliases[0].path.as_deref(), Some("example"));
    assert_eq!(instance.aliases[0].challenge.as_deref(), Some("healthz"));

    // After the catalogue's services, before what is no service of the hub.
    let order: Vec<&str> = request
        .hub
        .instances
        .iter()
        .map(|instance| instance.manifest.identifier.as_str())
        .collect();
    assert_eq!(
        order,
        [
            "live.arkitekt.rekuest",
            "live.arkitekt.mikro",
            "org.example.service",
            "live.arkitekt.s3"
        ]
    );
}

/// Every command that names a service by its compose service finds it: its image, a pin,
/// and the endpoint a hub is waited on.
#[test]
fn it_is_found_by_its_compose_service() {
    let mut config = hub();
    assert!(config.stack_images().contains(&(
        "example".to_string(),
        "registry.example.org/example:1".to_string()
    )));
    assert_eq!(config.service_at("example"), Some(example()));

    config.set_service_image("example", "registry.example.org/example:2");
    assert_eq!(
        config.service(example()).image.as_deref(),
        Some("registry.example.org/example:2")
    );
    // Nothing of the catalogue is behind for it, and nothing is caught up.
    assert_eq!(
        konstruktor_core::config::hub::caught_up_image(example(), "example:latest"),
        None
    );
    assert!(konstruktor_core::config::hub::is_supported_image(
        example(),
        "anything:at-all"
    ));

    let endpoints = konstruktor_core::ready::endpoints(&config).expect("a published port");
    let example = endpoints
        .iter()
        .find(|endpoint| endpoint.name == "example")
        .expect("it is waited on");
    assert!(example.url.ends_with("/example/healthz"), "{}", example.url);
}

/// An image cannot run under another service's name: its config is written by it for the
/// service it knows itself as.
#[test]
fn an_image_runs_under_the_name_it_gives_itself() {
    let config = hub();
    assert_eq!(
        konstruktor_core::contract::names_agree(&config, &said()),
        Ok(())
    );

    let mut swapped = said();
    swapped.insert(
        "example".to_string(),
        support::description_of(ServiceId::Mikro),
    );
    let refused = konstruktor_core::contract::names_agree(&config, &swapped).unwrap_err();
    assert!(
        refused.contains("`example`") && refused.contains("`mikro`"),
        "{refused}"
    );
}
