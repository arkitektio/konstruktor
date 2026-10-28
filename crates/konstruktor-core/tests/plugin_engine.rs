use konstruktor_core::catalog::ServiceId;
use konstruktor_core::config::hub::{
    HubConfig, HubConfigOptions, LokOptions, PluginEngineBlock, LOCAL_COORD_SERVER,
    PLUGIN_ENGINE_IDENTIFIER,
};
use konstruktor_core::connect::authorize::HubEnvelope;
use konstruktor_core::connect::manifest::{build_hub_request, AdvertisedHost, HubManifestOptions};
use konstruktor_core::create::{add_plugin_engine, confirm_plugin_engine, HubAnswers};
use konstruktor_core::generate::compose::PLUGIN_ENGINE_ENV_FILE;
use konstruktor_core::generate::lok::{build_access, preconfigured_hub};
use konstruktor_core::generate::{generate_hub_files, GeneratedFiles, IssuedIdentity};
use konstruktor_core::hosts::HostCategory;
use konstruktor_core::secrets::KeyPair;
use serde_norway::Value;

mod support;

// The plugin engine: the app that runs, beside a hub, the apps its organization installs
// through Kabinet. It is approved with the hub: the hub's manifest asks for it with a
// mandate key Konstruktor made, and the same token is in the engine's environment.

const DOCKER_SOCKET: &str = "/var/run/docker.sock";

fn signing_key() -> KeyPair {
    KeyPair {
        key_type: "RS256".into(),
        public_key: "-----BEGIN PUBLIC KEY-----\nPUBLIC\n-----END PUBLIC KEY-----\n".into(),
        private_key: "-----BEGIN PRIVATE KEY-----\nPRIVATE\n-----END PRIVATE KEY-----\n".into(),
    }
}

fn remote(services: Vec<ServiceId>) -> HubConfig {
    support::hub(&HubConfigOptions {
        device_id: "device".into(),
        coord_server: "https://go.arkitekt.live/".into(),
        services: Some(services),
        ..Default::default()
    })
}

fn self_contained(services: Vec<ServiceId>) -> HubConfig {
    support::hub(&HubConfigOptions {
        device_id: "device".into(),
        coord_server: LOCAL_COORD_SERVER.into(),
        services: Some(services),
        http_port: Some(7190),
        https_port: None,
        lok: Some(LokOptions {
            hub_identifier: "lab".into(),
            hosts: vec![AdvertisedHost {
                host: "localhost".into(),
                kind: HostCategory::Loopback,
            }],
            organization: "acme".into(),
            user: "ada".into(),
            user_password: Some("pass".into()),
            mandate_keys: vec!["token-one".into()],
            key_pair: Some(signing_key()),
        }),
        ..Default::default()
    })
}

fn with_engine(mut config: HubConfig) -> HubConfig {
    config.plugin_engine = Some(PluginEngineBlock::new("lab", DOCKER_SOCKET, None));
    config
}

fn kabinet_hub() -> Vec<ServiceId> {
    vec![ServiceId::Rekuest, ServiceId::Kabinet]
}

fn files_of(config: &HubConfig) -> GeneratedFiles {
    generate_hub_files(config, &IssuedIdentity::default(), &support::said())
}

fn compose_of(config: &HubConfig) -> Value {
    serde_norway::from_str(&files_of(config)["docker-compose.yaml"]).expect("valid YAML")
}

fn engine_of(config: &HubConfig) -> Value {
    compose_of(config)["services"]["plugin_engine"].clone()
}

fn request_options() -> HubManifestOptions {
    HubManifestOptions {
        identifier: "lab".into(),
        node_id: Some("device".into()),
        internal_host: Some("gateway".into()),
        described: support::said(),
        ..Default::default()
    }
}

fn answers(plugin_engine: bool) -> HubAnswers {
    serde_json::from_value(serde_json::json!({
        "dir": "/tmp/lab",
        "name": "lab",
        "coord_server": "https://go.arkitekt.live",
        "identifier": " lab ",
        "services": ["rekuest", "kabinet"],
        "hosts": [],
        "plugin_engine": plugin_engine,
    }))
    .expect("answers deserialize")
}

#[test]
fn the_engine_is_handed_the_socket_and_starts_apps_on_the_hubs_network() {
    let config = with_engine(remote(kabinet_hub()));
    let engine = engine_of(&config);

    assert_eq!(
        engine["image"],
        "ghcr.io/arkitektio/plugin_engine:3.0.0-vanilla"
    );
    assert_eq!(engine["command"][0], "arkitekt");
    let volumes: Vec<&str> = engine["volumes"]
        .as_sequence()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        volumes.contains(&"/var/run/docker.sock:/var/run/docker.sock"),
        "{volumes:?}"
    );
    assert!(
        volumes.contains(&"plugin_engine_state:/state"),
        "{volumes:?}"
    );

    let environment = &engine["environment"];
    assert_eq!(
        environment["ARKITEKT_NETWORK"],
        config.internal_network.as_str()
    );
    assert_eq!(engine["networks"][0], config.internal_network.as_str());
    assert_eq!(environment["KABINET_DEPLOYER_ID"], "lab");
    assert_eq!(environment["ARKITEKT_DEVICE_ID"], "device");
    assert_eq!(environment["DOCKER_HOST"], "unix:///var/run/docker.sock");
    assert_eq!(environment["XDG_STATE_HOME"], "/state");
    // Its session survives a restart.
    assert!(compose_of(&config)["volumes"]["plugin_engine_state"].is_mapping());
}

#[test]
fn a_rootless_daemons_socket_is_mounted_where_the_engine_looks() {
    let mut config = remote(kabinet_hub());
    config.plugin_engine = Some(PluginEngineBlock::new(
        "lab",
        DOCKER_SOCKET,
        Some("/run/user/1000/docker.sock".into()),
    ));

    assert_eq!(
        engine_of(&config)["volumes"][0],
        "/run/user/1000/docker.sock:/var/run/docker.sock"
    );
}

#[test]
fn the_mandate_key_is_in_a_file_of_its_own_and_not_in_the_compose_file() {
    let config = with_engine(remote(kabinet_hub()));
    let token = &config.plugin_engine.as_ref().unwrap().mandate_key;
    let files = files_of(&config);

    assert_eq!(
        files[PLUGIN_ENGINE_ENV_FILE],
        format!("FAKTS_MANDATE_KEY={token}\n")
    );
    assert!(!files["docker-compose.yaml"].contains(token.as_str()));
    assert_eq!(
        engine_of(&config)["env_file"][0],
        format!("./{PLUGIN_ENGINE_ENV_FILE}")
    );
}

#[test]
fn on_a_hub_accepted_elsewhere_the_engine_logs_in_where_the_hub_was_accepted() {
    let environment = engine_of(&with_engine(remote(kabinet_hub())))["environment"].clone();

    assert_eq!(environment["FAKTS_URL"], "https://go.arkitekt.live");
    assert_eq!(environment["ARKITEKT_GATEWAY"], "https://go.arkitekt.live");
    assert!(environment.get("FAKTS_ALLOW_INSECURE_TRANSPORT").is_none());
}

#[test]
fn the_hub_asks_to_be_approved_with_its_engine() {
    let config = with_engine(remote(kabinet_hub()));
    let request = build_hub_request(&config, &request_options());

    let [client] = request.hub.clients.as_slice() else {
        panic!("one client request, got {:?}", request.hub.clients);
    };
    assert_eq!(client.identifier, "plugin_engine");
    assert_eq!(
        client.mandate_key,
        config.plugin_engine.as_ref().unwrap().mandate_key
    );
    assert_eq!(client.manifest.identifier, PLUGIN_ENGINE_IDENTIFIER);
    assert_eq!(client.manifest.version, "3.0.0");
    // What its Kabinet and Rekuest clients ask for, and nothing generic.
    assert_eq!(
        client.manifest.scopes,
        [
            "kabinet_approve",
            "kabinet_deploy",
            "kabinet_read",
            "kabinet_repos",
            "rekuest_agent",
            "rekuest_call",
            "rekuest_read",
            "rekuest_write"
        ]
    );
    assert_eq!(client.manifest.device_id.as_deref(), Some("device"));
    let services: Vec<&str> = client
        .manifest
        .requirements
        .iter()
        .map(|r| r.service.as_str())
        .collect();
    assert_eq!(
        services,
        [
            "live.arkitekt.kabinet",
            "live.arkitekt.rekuest",
            "live.arkitekt.s3"
        ]
    );
    // Lok refuses a proposed secret shorter than this.
    assert!(client.mandate_key.len() >= 32);
}

#[test]
fn a_self_contained_hubs_own_lok_approves_the_engine_on_boot() {
    let config = with_engine(self_contained(kabinet_hub()));
    let lok = config.running_lok().unwrap();
    let token = config.plugin_engine.as_ref().unwrap().mandate_key.clone();

    let hub = preconfigured_hub(&config, lok, &support::said());
    assert_eq!(hub.clients.len(), 1);
    assert_eq!(hub.clients[0].mandate_key, token);

    // Over the hub's own network, in plain HTTP, which fakts has to be told it may use.
    let environment = engine_of(&config)["environment"].clone();
    assert_eq!(environment["FAKTS_URL"], "http://gateway");
    assert_eq!(environment["FAKTS_ALLOW_INSECURE_TRANSPORT"], "1");
    let depends_on = engine_of(&config)["depends_on"].clone();
    assert!(depends_on
        .as_sequence()
        .unwrap()
        .contains(&Value::from("lok")));

    // The engine's token is the hub's, not one of the tokens handed to apps.
    let access = build_access(&config, lok);
    assert_eq!(access["mandate_keys"], serde_json::json!(["token-one"]));
}

#[test]
fn without_kabinet_there_is_no_engine_and_nothing_is_asked_for() {
    let config = with_engine(remote(vec![ServiceId::Rekuest, ServiceId::Mikro]));

    assert!(config.running_plugin_engine().is_none());
    assert!(engine_of(&config).is_null());
    assert!(!files_of(&config).contains_key(PLUGIN_ENGINE_ENV_FILE));
    assert!(build_hub_request(&config, &request_options())
        .hub
        .clients
        .is_empty());
}

#[test]
fn a_hub_without_an_engine_writes_what_it_always_did() {
    let config = remote(kabinet_hub());
    assert!(engine_of(&config).is_null());
    assert!(build_hub_request(&config, &request_options())
        .hub
        .clients
        .is_empty());
}

#[test]
fn creating_a_hub_adds_the_engine_unless_it_is_left_out() {
    let mut config = remote(kabinet_hub());
    add_plugin_engine(&mut config, &answers(true), DOCKER_SOCKET, None);
    let engine = config.plugin_engine.as_ref().expect("an engine");
    assert_eq!(engine.deployer_id, "lab");

    let mut left_out = remote(kabinet_hub());
    add_plugin_engine(&mut left_out, &answers(false), DOCKER_SOCKET, None);
    assert!(left_out.plugin_engine.is_none());

    let mut without_kabinet = remote(vec![ServiceId::Rekuest]);
    add_plugin_engine(&mut without_kabinet, &answers(true), DOCKER_SOCKET, None);
    assert!(without_kabinet.plugin_engine.is_none());
}

#[test]
fn the_engine_is_asked_for_unless_answers_say_otherwise() {
    let answers: HubAnswers = serde_json::from_value(serde_json::json!({
        "dir": "/tmp/lab", "name": "lab", "coord_server": "local", "identifier": "lab",
        "services": [], "hosts": [],
    }))
    .unwrap();
    assert!(answers.plugin_engine);
}

#[test]
fn the_engines_image_is_reported_and_written_back() {
    let mut config = with_engine(remote(kabinet_hub()));
    assert!(config.stack_images().contains(&(
        "plugin_engine".to_string(),
        "ghcr.io/arkitektio/plugin_engine:3.0.0-vanilla".to_string()
    )));

    config.set_service_image(
        "plugin_engine",
        "ghcr.io/arkitektio/plugin_engine:1.2.0-vanilla",
    );
    let engine = config.plugin_engine.as_ref().unwrap();
    assert_eq!(
        engine.image,
        "ghcr.io/arkitektio/plugin_engine:1.2.0-vanilla"
    );
    assert_eq!(engine.version(), "1.2.0");
}

#[test]
fn a_version_is_read_off_any_tag_and_never_off_a_registry_port() {
    let mut engine = PluginEngineBlock::new("lab", DOCKER_SOCKET, None);
    for (image, version) in [
        ("ghcr.io/arkitektio/plugin_engine:2.3.4-vanilla", "2.3.4"),
        ("localhost:5000/plugin_engine:1.0.0", "1.0.0"),
        ("localhost:5000/plugin_engine", "0.0.0"),
        ("ghcr.io/arkitektio/plugin_engine:main", "0.0.0"),
        (
            "ghcr.io/arkitektio/plugin_engine:1.0.1-vanilla@sha256:abc",
            "1.0.1",
        ),
    ] {
        engine.image = image.into();
        assert_eq!(engine.version(), version, "{image}");
    }
}

#[test]
fn the_profile_keeps_the_engine_and_regenerates_the_same_files() {
    let config = with_engine(self_contained(kabinet_hub()));
    let read_back: HubConfig =
        serde_norway::from_str(&serde_norway::to_string(&config).unwrap()).unwrap();
    assert_eq!(read_back.plugin_engine, config.plugin_engine);
    assert_eq!(files_of(&read_back), files_of(&config));
}

/// What the coordination server names an approved key by: never the key itself.
fn sha256_hex(key: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(key.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn envelope(client_requests: serde_json::Value) -> HubEnvelope {
    serde_json::from_value(serde_json::json!({
        "token_type": "Bearer",
        "access_token": "eyJ",
        "client_id": "hub-client",
        "client_requests": client_requests,
    }))
    .expect("an envelope")
}

#[test]
fn an_engine_the_server_approved_with_the_hub_is_kept() {
    let mut config = with_engine(remote(kabinet_hub()));
    let key = config.plugin_engine.as_ref().unwrap().mandate_key.clone();
    let approved = envelope(serde_json::json!({
        "plugin_engine": {"mandate": "1", "key_sha256": sha256_hex(&key), "client_ids": []}
    }));

    assert_eq!(confirm_plugin_engine(&mut config, &approved), None);
    assert!(config.running_plugin_engine().is_some());
}

/// A hub created unconnected wants its engine but does not run it: nobody has approved its
/// client yet, and an engine without one fails its login on every start. Connecting asks
/// for the client, and the approval switches the engine on.
#[test]
fn an_unconnected_hubs_engine_runs_once_the_server_approved_it() {
    let mut config = with_engine(remote(kabinet_hub()));
    config.plugin_engine.as_mut().unwrap().enabled = false;
    assert!(config.running_plugin_engine().is_none());
    assert!(
        engine_of(&config).is_null(),
        "an unapproved engine is in the stack"
    );
    assert!(
        konstruktor_core::connect::manifest::client_requests(&config, None)
            .iter()
            .any(|request| request.identifier == "plugin_engine"),
        "connecting does not ask for the engine's client"
    );

    let key = config.plugin_engine.as_ref().unwrap().mandate_key.clone();
    let approved = envelope(serde_json::json!({
        "plugin_engine": {"mandate": "1", "key_sha256": sha256_hex(&key), "client_ids": []}
    }));
    assert_eq!(confirm_plugin_engine(&mut config, &approved), None);
    assert!(config.running_plugin_engine().is_some());
    assert!(!engine_of(&config).is_null());
}

/// A server from before client requests ignores the request: an engine left in the stack
/// would fail its login on every start.
#[test]
fn an_engine_the_server_never_approved_is_left_out_and_says_so() {
    for answer in [
        serde_json::json!({}),
        serde_json::json!({"plugin_engine": {"mandate": "1", "key_sha256": sha256_hex("another-secret")}}),
    ] {
        let mut config = with_engine(remote(kabinet_hub()));
        let warning = confirm_plugin_engine(&mut config, &envelope(answer));

        assert!(warning.is_some_and(|w| w.contains("did not approve the plugin engine")));
        assert!(config.plugin_engine.is_none());
        assert!(engine_of(&config).is_null());
    }
}

#[test]
fn an_envelope_from_before_client_requests_still_reads() {
    let old: HubEnvelope = serde_json::from_value(serde_json::json!({
        "token_type": "Bearer", "access_token": "eyJ", "client_id": "hub-client",
        "clients": {"abc": {"client_id": "abc"}},
    }))
    .unwrap();
    assert!(old.client_requests.is_empty());
    assert!(!old.approves_client("plugin_engine", "anything"));
}
