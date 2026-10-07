use konstruktor_core::catalog::ServiceId;
use konstruktor_core::config::hub::{HubConfig, HubConfigOptions, LokOptions, LOCAL_COORD_SERVER};
use konstruktor_core::connect::manifest::AdvertisedHost;
use konstruktor_core::generate::lok::{build_access, preconfigured_hub, ACCESS_FILE};
use konstruktor_core::generate::{generate_hub_files, GeneratedFiles, IssuedIdentity};
use konstruktor_core::hosts::HostCategory;
use konstruktor_core::secrets::KeyPair;
use serde_norway::Value;

mod support;

// A self-contained hub: one that runs its own coordination server.
//
// There is no golden fixture for it — the Python generator this crate was ported from
// wrote a different stack, with different key names, that nothing reads any more. The
// oracle here is Lok itself: every assertion below pins a rule its settings model or its
// boot commands hold, named where it is held, because the failure mode of getting one
// wrong is not an error. Lok ignores keys it does not know and boots anyway.

const PORT: u16 = 7190;

/// A fixed pair, so no test waits for a prime search. The contents are never parsed here.
fn signing_key() -> KeyPair {
    KeyPair {
        key_type: "RS256".into(),
        public_key: "-----BEGIN PUBLIC KEY-----\nPUBLIC\n-----END PUBLIC KEY-----\n".into(),
        private_key: "-----BEGIN PRIVATE KEY-----\nPRIVATE\n-----END PRIVATE KEY-----\n".into(),
    }
}

fn self_contained(services: Vec<ServiceId>) -> HubConfig {
    support::hub(&HubConfigOptions {
        device_id: "device".into(),
        coord_server: LOCAL_COORD_SERVER.into(),
        services: Some(services),
        http_port: Some(PORT),
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
            redeem_tokens: vec!["token-one".into(), "token-two".into()],
            key_pair: Some(signing_key()),
        }),
        ..Default::default()
    })
}

fn default_hub() -> HubConfig {
    self_contained(vec![ServiceId::Rekuest, ServiceId::Mikro])
}

fn files_of(config: &HubConfig) -> GeneratedFiles {
    generate_hub_files(config, &IssuedIdentity::default(), &support::said())
}

fn yaml(files: &GeneratedFiles, name: &str) -> Value {
    serde_norway::from_str(
        files
            .get(name)
            .unwrap_or_else(|| panic!("{name} is generated")),
    )
    .unwrap_or_else(|e| panic!("{name} parses: {e}"))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_sequence()
        .expect("a sequence")
        .iter()
        .map(|v| v.as_str().expect("a string").to_string())
        .collect()
}

/// The issuer is a name. Lok is told to advertise its endpoints wherever a request
/// arrived, so nothing a token carries depends on which address it was got at — which is
/// what lets the same hub be logged into from this machine, from another one, and from a
/// container on the stack's own network.
#[test]
fn the_issuer_is_a_name_and_login_follows_the_request() {
    let config = default_hub();
    let lok = config.running_lok().expect("a self-contained hub runs lok");
    assert_eq!(lok.issuer, "lok");

    let config = yaml(&files_of(&config), "configs/lok.yaml");
    assert_eq!(config["oidc_issuer"], Value::from("lok"));
    assert_eq!(config["discovery_follows_request"], Value::from(true));
    assert_eq!(config["django"]["force_script_name"], Value::from("lok"));
    // `http://lok` — the compose-internal name as an *address* — is what the Python
    // generator handed out, and what no app on the host could ever open.
    assert!(!config["oidc_issuer"].as_str().unwrap().contains("://"));
}

/// Neither the port nor the advertised address is part of what a token says.
#[test]
fn the_issuer_does_not_depend_on_where_the_hub_is_published() {
    let elsewhere = support::hub(&HubConfigOptions {
        coord_server: LOCAL_COORD_SERVER.into(),
        http_port: Some(80),
        https_port: None,
        lok: Some(LokOptions {
            hosts: vec![AdvertisedHost {
                host: "192.168.1.5".into(),
                kind: HostCategory::Private,
            }],
            key_pair: Some(signing_key()),
            ..Default::default()
        }),
        ..Default::default()
    });
    assert_eq!(elsewhere.running_lok().unwrap().issuer, "lok");
}

/// The services verify with Lok's key inline. A `jwks_uri` would be an address to fetch,
/// which from inside the stack's network is the container asking.
///
/// A service's image writes its own config from the hub's facts, so the facts are where
/// this is said; Lok's config is still written here.
#[test]
fn every_service_trusts_the_stacks_own_signing_key() {
    let config = default_hub();
    let told = |id: ServiceId| {
        konstruktor_core::contract::facts(
            &config,
            id,
            &IssuedIdentity::default(),
            &Default::default(),
        )["hub"]["auth"]
            .clone()
    };
    let trusted = [
        ("rekuest", told(ServiceId::Rekuest)),
        ("mikro", told(ServiceId::Mikro)),
        (
            "lok",
            yaml(&files_of(&config), "configs/lok.yaml")["authentikate"].clone(),
        ),
    ];
    for (name, auth) in trusted {
        let issuers = auth["issuers"].as_sequence().expect("issuers").clone();
        assert_eq!(issuers.len(), 1, "{name}");
        let issuer = &issuers[0];
        assert_eq!(issuer["kind"], Value::from("rsa"), "{name}");
        assert_eq!(issuer["iss"], Value::from("lok"), "{name}");
        // The `kid` Lok stamps into every token header (`LokSettings.key_id`).
        assert_eq!(issuer["kid"], Value::from("lok-key-1"), "{name}");
        assert_eq!(
            issuer["public_key"],
            Value::from(signing_key().public_key),
            "{name}"
        );
        assert!(issuer.get("jwks_uri").is_none(), "{name}");
    }
}

#[test]
fn lok_signs_with_the_private_half_and_advertises_the_public_one() {
    let config = yaml(&files_of(&default_hub()), "configs/lok.yaml");
    assert_eq!(
        config["private_key"],
        Value::from(signing_key().private_key)
    );
    assert_eq!(
        config["lok"]["public_key"],
        Value::from(signing_key().public_key)
    );
    assert_eq!(config["lok"]["key_id"], Value::from("lok-key-1"));
    // A plain-HTTP gateway: without this Lok refuses every OAuth request.
    assert_eq!(
        config["django"]["allow_insecure_transport"],
        Value::from(true)
    );
}

/// The chain `run.sh` walks: partners, users, organizations (which register the partner's
/// hub), memberships, tokens. Each step looks the previous one up by name.
#[test]
fn the_seed_names_line_up_the_way_lok_looks_them_up() {
    let config = yaml(&files_of(&default_hub()), "configs/lok.yaml");

    let user = &config["users"][0];
    assert_eq!(user["username"], Value::from("ada"));
    assert_eq!(user["password"], Value::from("pass"));

    let organization = &config["organizations"][0];
    assert_eq!(organization["identifier"], Value::from("acme"));
    assert_eq!(organization["owner"], Value::from("ada"));
    assert_eq!(organization["auto_configure"], Value::from(true));

    let membership = &config["memberships"][0];
    assert_eq!(membership["user"], Value::from("ada"));
    assert_eq!(membership["organization"], Value::from("acme"));
    // A role Lok creates with every organization; one it does not know raises.
    assert_eq!(strings(&membership["roles"]), ["admin"]);

    let partner = &config["kommunity_partners"][0];
    // Off by default in Lok, and without it the hub is never registered.
    assert_eq!(partner["auto_configure"], Value::from(true));
    assert_eq!(
        partner["preconfigured_hub"]["identifier"],
        Value::from("lab")
    );

    let tokens = config["redeem_tokens"].as_sequence().expect("tokens");
    assert_eq!(tokens.len(), 2);
    for (token, expected) in tokens.iter().zip(["token-one", "token-two"]) {
        assert_eq!(token["token"], Value::from(expected));
        assert_eq!(token["user"], Value::from("ada"));
        assert_eq!(token["organization"], Value::from("acme"));
        // `ensuretokens` looks the hub up by this, in that organization.
        assert_eq!(token["hub"], partner["preconfigured_hub"]["identifier"]);
        // Present and null: left out, a token expires after 90 days.
        assert_eq!(token["expires_in_days"], Value::Null);
        assert_eq!(token["max_redemptions"], Value::Null);
    }
}

#[test]
fn the_hub_advertises_every_service_the_store_and_lok_at_the_gateway() {
    let config = default_hub();
    let hub = preconfigured_hub(&config, config.running_lok().unwrap(), &support::said());

    let advertised: Vec<&str> = hub
        .instances
        .iter()
        .map(|i| i.manifest.identifier.as_str())
        .collect();
    assert_eq!(
        advertised,
        [
            "live.arkitekt.rekuest",
            "live.arkitekt.mikro",
            // What every service that stores objects makes its clients require.
            "live.arkitekt.s3",
            "live.arkitekt.lok",
        ]
    );

    for instance in &hub.instances {
        let reachable = &instance.aliases[0];
        assert_eq!(
            reachable.host, "localhost",
            "{}",
            instance.manifest.identifier
        );
        assert_eq!(reachable.port, PORT, "{}", instance.manifest.identifier);
        assert_eq!(reachable.kind, "absolute");
        assert!(!reachable.ssl);
        // Beside it, the gateway by name — for apps on the stack's own network.
        assert_eq!(instance.aliases[1].host, "gateway");
        assert_eq!(instance.aliases[1].port, 80);
        // Told apart by kind: a client in that network tries it first, any other last.
        assert_eq!(instance.aliases[1].kind, "docker");
    }

    let path_of = |identifier: &str| {
        hub.instances
            .iter()
            .find(|i| i.manifest.identifier == identifier)
            .and_then(|i| i.aliases[0].path.clone())
    };
    assert_eq!(path_of("live.arkitekt.mikro").as_deref(), Some("mikro"));
    assert_eq!(path_of("live.arkitekt.lok").as_deref(), Some("lok"));
    assert_eq!(path_of("live.arkitekt.s3"), None);
}

/// Lok hands an instance's key to clients as its *challenge key*, and a client that gets
/// one demands a signed answer from the alias's health check. No service signs one, so a
/// key here is every alias failing its challenge — which is what happened.
#[test]
fn no_instance_pins_a_key_its_health_check_cannot_answer_for() {
    let config = default_hub();
    assert!(
        config
            .service(konstruktor_core::catalog::ServiceId::Rekuest)
            .instance_key_pair
            .is_some(),
        "the services still have theirs"
    );
    let hub = preconfigured_hub(&config, config.running_lok().unwrap(), &support::said());
    for instance in &hub.instances {
        assert!(
            instance.manifest.challenge_key.is_none(),
            "{}",
            instance.manifest.identifier
        );
    }
}

#[test]
fn the_stack_runs_lok_with_a_database_and_a_bucket_of_its_own() {
    let files = files_of(&default_hub());
    let compose = yaml(&files, "docker-compose.yaml");
    let services = &compose["services"];

    let lok = &services["lok"];
    assert_eq!(lok["image"], Value::from("jhnnsrs/lok:latest"));
    // Served by its image's own command: none is written for it.
    assert!(lok.get("command").is_none());
    assert_eq!(
        strings(&lok["volumes"]),
        ["./configs/lok.yaml:/workspace/config.yaml"]
    );

    let databases = services["db"]["environment"]["POSTGRES_MULTIPLE_DATABASES"]
        .as_str()
        .unwrap();
    assert_eq!(databases, "rekuest_main,mikro_main,lok_main");

    let buckets: Vec<String> = yaml(&files, "configs/rustfs_init.yaml")["buckets"]
        .as_sequence()
        .unwrap()
        .iter()
        .map(|b| b["name"].as_str().unwrap().to_string())
        .collect();
    assert!(buckets.contains(&"lokmedia".to_string()), "{buckets:?}");

    // One port, on a hub that serves plain HTTP.
    assert_eq!(
        strings(&services["gateway"]["ports"]),
        [format!("{PORT}:80")]
    );
}

/// A coordination server alone still needs somewhere to keep its sessions and its media.
#[test]
fn lok_brings_its_infrastructure_even_with_no_other_service() {
    let config = support::hub(&HubConfigOptions {
        coord_server: LOCAL_COORD_SERVER.into(),
        rekuest_server: "none".into(),
        services: Some(Vec::new()),
        lok: Some(LokOptions {
            key_pair: Some(signing_key()),
            ..Default::default()
        }),
        ..Default::default()
    });
    let compose = yaml(&files_of(&config), "docker-compose.yaml");
    for service in ["lok", "db", "redis", "rustfs", "rustfs_init", "gateway"] {
        assert!(compose["services"].get(service).is_some(), "{service}");
    }
}

#[test]
fn the_gateway_routes_lok_and_serves_its_well_known_from_the_root() {
    let caddyfile = files_of(&default_hub())
        .remove("configs/Caddyfile")
        .unwrap();
    // Exactly `/lok` and what is under it. `/lok*` would also take `/lokate`.
    assert!(
        caddyfile.contains("\t@lok path /lok /lok/*\n"),
        "{caddyfile}"
    );
    assert!(!caddyfile.contains("path /lok*"), "{caddyfile}");
    assert!(
        caddyfile.contains(
            "\t@wellknown path /.well-known/*\n\thandle @wellknown {\n\t\trewrite * /lok{uri}\n\t\treverse_proxy lok:80\n\t}\n"
        ),
        "{caddyfile}"
    );
    assert!(
        caddyfile.contains("\t@lokmedia path /lokmedia*\n"),
        "{caddyfile}"
    );
}

/// The byte-compared Caddyfile of every other hub is what it was.
#[test]
fn a_hub_that_trusts_a_remote_server_gets_none_of_it() {
    let config = support::hub(&HubConfigOptions {
        coord_server: "go.arkitekt.live".into(),
        ..Default::default()
    });
    assert!(config.lok.is_none());

    let profile = serde_norway::to_string(&config).unwrap();
    assert!(
        !profile.contains("\nlok:"),
        "upstream's model has no such key"
    );

    let files = files_of(&config);
    assert!(!files.contains_key("configs/lok.yaml"));
    assert!(!files.contains_key(ACCESS_FILE));
    let caddyfile = &files["configs/Caddyfile"];
    assert!(!caddyfile.contains("@lok"), "{caddyfile}");
    assert!(!caddyfile.contains(".well-known"), "{caddyfile}");
    assert!(yaml(&files, "docker-compose.yaml")["services"]
        .get("lok")
        .is_none());
}

/// What makes `update` and `rollback` safe: they regenerate from the profile alone, with
/// no grant to read an issuer from — so the issuer has to survive the round trip.
#[test]
fn regenerating_from_the_stored_profile_changes_nothing() {
    let config = default_hub();
    let read_back: HubConfig =
        serde_norway::from_str(&serde_norway::to_string(&config).unwrap()).unwrap();
    assert_eq!(read_back.lok, config.lok);
    assert_eq!(files_of(&read_back), files_of(&config));
}

#[test]
fn loks_image_can_be_reported_and_written_back() {
    let mut config = default_hub();
    assert!(config
        .stack_images()
        .contains(&("lok".to_string(), "jhnnsrs/lok:latest".to_string())));
    config.set_service_image("lok", "jhnnsrs/lok:1.2.3");
    assert_eq!(config.running_lok().unwrap().image, "jhnnsrs/lok:1.2.3");
}

#[test]
fn the_access_document_says_how_to_reach_the_hub() {
    let config = default_hub();
    let access = build_access(&config, config.running_lok().unwrap());
    let base = format!("http://localhost:{PORT}");

    assert_eq!(access["fakts_url"], base);
    assert_eq!(access["gateway_url"], base);
    assert_eq!(access["issuer"], "lok");
    assert_eq!(access["hub"], "lab");
    assert_eq!(access["organization"], "acme");
    assert_eq!(access["users"][0]["username"], "ada");
    assert_eq!(access["users"][0]["password"], "pass");
    assert_eq!(
        access["redeem_tokens"],
        serde_json::json!(["token-one", "token-two"])
    );
    assert_eq!(access["services"]["mikro"]["url"], format!("{base}/mikro"));
    assert_eq!(
        access["services"]["mikro"]["identifier"],
        "live.arkitekt.mikro"
    );
    assert_eq!(access["services"]["lok"]["url"], format!("{base}/lok"));

    // The file on disk is that document, and nothing else writes it.
    let written: serde_json::Value = serde_json::from_str(&files_of(&config)[ACCESS_FILE]).unwrap();
    assert_eq!(written, access);
}

/// A script on this machine is handed `localhost` whenever the hub advertises it, wherever
/// it comes in the list — the local-only preset puts `127.0.0.1` first.
#[test]
fn the_access_document_prefers_localhost_among_the_advertised_addresses() {
    let host = |host: &str, kind| AdvertisedHost {
        host: host.into(),
        kind,
    };
    let config = support::hub(&HubConfigOptions {
        coord_server: LOCAL_COORD_SERVER.into(),
        http_port: Some(PORT),
        https_port: None,
        lok: Some(LokOptions {
            hosts: vec![
                host("127.0.0.1", HostCategory::Loopback),
                host("localhost", HostCategory::Loopback),
            ],
            key_pair: Some(signing_key()),
            ..Default::default()
        }),
        ..Default::default()
    });
    let access = build_access(&config, config.running_lok().unwrap());
    assert_eq!(access["fakts_url"], format!("http://localhost:{PORT}"));

    let lan = support::hub(&HubConfigOptions {
        coord_server: LOCAL_COORD_SERVER.into(),
        http_port: Some(PORT),
        https_port: None,
        lok: Some(LokOptions {
            hosts: vec![host("192.168.1.5", HostCategory::Private)],
            key_pair: Some(signing_key()),
            ..Default::default()
        }),
        ..Default::default()
    });
    let access = build_access(&lan, lan.running_lok().unwrap());
    assert_eq!(access["fakts_url"], format!("http://192.168.1.5:{PORT}"));
}
