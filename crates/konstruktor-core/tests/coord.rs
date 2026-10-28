//! A coordination server, as generated: what Lok is configured as, what the gateway serves,
//! and what the stack runs and keeps running.
//!
//! The profile is a fixture (`fixtures/coord_config.yaml`) so the goldens are stable: a
//! coordination server's secrets are minted fresh every time it is built. Bless both with
//! `KONSTRUKTOR_BLESS_GOLDEN=1 cargo test -p konstruktor-core --test coord`; a fixture that is
//! missing is built then.

use std::collections::BTreeMap;
use std::path::PathBuf;

use konstruktor_core::config::coord::{build_coord_config, CoordOptions, MeshChoice, SignupPolicy};
use konstruktor_core::config::hub::{HubConfig, PublicSite, PublicTls, StorageMode};
use konstruktor_core::contract::{Description, Job, Needs, Said, Sidecar, CONTRACT};
use konstruktor_core::generate::{generate_hub_files, GeneratedFiles};
use serde_norway::Value;

const BLESS: &str = "KONSTRUKTOR_BLESS_GOLDEN";

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// What Lok 8's image says of itself (`konstruktor inspect --image jhnnsrs/lok:8`), as far
/// as a coordination server is written from it.
fn lok_said() -> Said {
    let job = |command: &[&str]| Job {
        command: command.iter().map(|c| c.to_string()).collect(),
        ..Default::default()
    };
    let description = Description {
        contract: CONTRACT,
        name: "lok".into(),
        identifier: "live.arkitekt.lok".into(),
        serve: vec!["arkitekt-service".into(), "serve".into()],
        jobs: BTreeMap::from([
            (
                "migrate".to_string(),
                job(&["arkitekt-service", "run", "migrate"]),
            ),
            (
                "reconcile_meshes".to_string(),
                job(&["arkitekt-service", "run", "reconcile_meshes"]),
            ),
        ]),
        prepare: Some("migrate".into()),
        sidecars: vec![Sidecar {
            name: "ionskale".into(),
            image: "jhnnsrs/ionskale:latest".into(),
            summary: String::new(),
            optional: true,
        }],
        needs: Needs {
            storage: vec!["media".into()],
            ..Default::default()
        },
        ..Default::default()
    };
    Said::from([("lok".to_string(), description)])
}

fn options(tls: Option<PublicTls>, mesh: bool) -> CoordOptions {
    let https = tls.is_some();
    CoordOptions {
        device_id: "device-1".into(),
        name: "Example Lab".into(),
        origin: if https {
            "https://coord.example.org".into()
        } else {
            "http://localhost:7180".into()
        },
        public: tls.map(|tls| PublicSite {
            domain: "coord.example.org".into(),
            extra_names: if mesh {
                vec!["mesh.coord.example.org".into()]
            } else {
                Vec::new()
            },
            tls,
        }),
        http_port: Some(if https { 80 } else { 7180 }),
        https_port: https.then_some(443),
        admin: "admin".into(),
        admin_password: Some("admin-password".into()),
        admin_email: Some("ops@example.org".into()),
        organization: "Example Lab".into(),
        mesh: mesh.then(|| MeshChoice {
            domain: "mesh.coord.example.org".into(),
            dns_suffix: None,
        }),
        email: None,
        signup: SignupPolicy::Closed,
        storage: StorageMode::default(),
        extra_roots: None,
        key_pair: None,
    }
}

fn acme() -> PublicTls {
    PublicTls::Acme {
        email: "ops@example.org".into(),
        ca: None,
        ca_root: None,
    }
}

/// The fixture profile: an ACME coordination server with a mesh.
fn fixture() -> HubConfig {
    let path = fixtures().join("coord_config.yaml");
    if !path.exists() && std::env::var(BLESS).as_deref() == Ok("1") {
        let mut config = build_coord_config(&options(Some(acme()), true));
        config.place_certificate(&[]).unwrap();
        let profile = konstruktor_core::profile::hub_profile(config);
        std::fs::write(&path, serde_norway::to_string(&profile).unwrap()).unwrap();
    }
    let text = std::fs::read_to_string(&path).expect("fixtures/coord_config.yaml is readable");
    let profile: Value = serde_norway::from_str(&text).expect("the fixture parses");
    serde_norway::from_value(profile["config"].clone()).expect("the fixture is a profile")
}

fn golden() -> GeneratedFiles {
    let root = fixtures().join("golden/coord");
    let mut out = GeneratedFiles::new();
    fn walk(dir: &std::path::Path, prefix: &str, out: &mut GeneratedFiles) {
        for entry in std::fs::read_dir(dir).expect("the golden dir is readable") {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().to_string();
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), &relative, out);
            } else {
                out.insert(relative, std::fs::read_to_string(entry.path()).unwrap());
            }
        }
    }
    walk(&root, "", &mut out);
    out
}

fn files_of(config: &HubConfig) -> GeneratedFiles {
    generate_hub_files(config, &Default::default(), &lok_said())
}

fn yaml(files: &GeneratedFiles, name: &str) -> Value {
    serde_norway::from_str(&files[name]).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn a_coordination_server_matches_its_goldens() {
    let generated = files_of(&fixture());
    if std::env::var(BLESS).as_deref() == Ok("1") {
        let root = fixtures().join("golden/coord");
        for (name, contents) in &generated {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
        }
    }
    let expected = golden();
    assert_eq!(
        generated.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>(),
        "a different set of files"
    );
    for (name, contents) in &expected {
        if name.ends_with(".yaml") {
            assert_eq!(
                yaml(&generated, name),
                yaml(&expected, name),
                "{name} differs"
            );
        } else {
            assert_eq!(&generated[name], contents, "{name} is not byte-identical");
        }
    }
}

#[test]
fn lok_issues_tokens_as_its_origin_and_answers_nowhere_else() {
    let files = files_of(&fixture());
    let lok = yaml(&files, "configs/lok.yaml");
    assert_eq!(lok["oidc_issuer"], "https://coord.example.org");
    assert_eq!(lok["discovery_follows_request"], false);
    assert_eq!(lok["kontrol_frontend_url"], "https://coord.example.org");
    assert_eq!(lok["django"]["allow_insecure_transport"], false);
    assert_eq!(
        lok["django"]["hosts"],
        serde_norway::from_str::<Value>("[coord.example.org, gateway, lok, localhost]").unwrap()
    );
    assert_eq!(
        lok["deployment"]["hub_configure_url"],
        "https://coord.example.org/hubconfigure/{code}"
    );
    assert_eq!(lok["account"]["allow_signup"], false);
    // No hub of its own, and no key anybody logs in with unasked.
    assert!(lok.get("kommunity_partners").is_none());
    assert!(lok.get("mandate_keys").is_none());
    let admin = &lok["users"][0];
    assert_eq!(admin["username"], "admin");
    assert_eq!(admin["is_superuser"], true);
    assert_eq!(lok["organizations"][0]["owner"], "admin");
    assert_eq!(lok["organizations"][0]["auto_configure"], false);
}

#[test]
fn lok_drives_the_mesh_and_the_mesh_logs_in_through_lok() {
    let config = fixture();
    let files = files_of(&config);
    let lok = yaml(&files, "configs/lok.yaml");
    let mesh = yaml(&files, "configs/ionskale.yaml");
    let token = lok["ionscale"]["service_token"].as_str().unwrap();
    assert!(token.starts_with("svc_"));
    assert_eq!(mesh["auth"]["service_tokens"][0]["token"], token);
    assert_eq!(lok["ionscale"]["server_url"], "http://ionskale:8080");
    assert_eq!(
        lok["ionscale"]["coord_url"],
        "https://mesh.coord.example.org"
    );
    assert_eq!(
        mesh["auth"]["provider"]["issuer"],
        "https://coord.example.org"
    );
    assert_eq!(
        mesh["auth"]["provider"]["client_secret"],
        lok["openid_apps"][0]["client_secret"]
    );
    assert_eq!(
        lok["openid_apps"][0]["redirect_uris"][0],
        "https://mesh.coord.example.org/a/callback"
    );
    assert_eq!(mesh["public_addr"], "mesh.coord.example.org:443");
    assert_eq!(mesh["tls"]["disable"], true);
    assert_eq!(
        mesh["dns"]["magic_dns_suffix"],
        lok["ionscale"]["magic_dns_suffix"]
    );
}

#[test]
fn the_gateway_serves_both_names_with_acme_and_everything_else_with_its_own() {
    let files = files_of(&fixture());
    let caddy = &files["configs/Caddyfile"];
    assert!(caddy.contains("\temail ops@example.org\n"), "{caddy}");
    assert!(
        caddy.contains("\ncoord.example.org {\n\timport coord\n}\n"),
        "{caddy}"
    );
    assert!(
        caddy.contains("\nmesh.coord.example.org {\n\treverse_proxy ionskale:8080\n}\n"),
        "{caddy}"
    );
    assert!(caddy.contains(
        "\nhttps:// {\n\ttls /etc/caddy/tls/hub.crt /etc/caddy/tls/hub.key\n\timport coord\n}\n"
    ));
    // The account frontend takes the root, after everything else.
    let kontrol = caddy.find("reverse_proxy kontrol:80").unwrap();
    let lok = caddy.find("reverse_proxy lok:80").unwrap();
    assert!(lok < kontrol);
    // No echoing CORS of the gateway's on Lok's session routes.
    assert!(!caddy.contains("Access-Control-Allow-Origin"), "{caddy}");
    assert!(!caddy.contains("auto_https"), "redirects stay on: {caddy}");
    assert!(
        !caddy.contains("redir"),
        "Caddy's own redirect is right on 443: {caddy}"
    );
}

#[test]
fn a_provided_certificate_is_served_from_the_folder() {
    let mut config = build_coord_config(&options(
        Some(PublicTls::Provided {
            cert_path: "/etc/letsencrypt/live/coord/fullchain.pem".into(),
            key_path: "/etc/letsencrypt/live/coord/privkey.pem".into(),
        }),
        false,
    ));
    // Published on another port of the host, served on the container's 443 all the same.
    config.gateway.exposed_https_port = Some(7443);
    let files = files_of(&config);
    let caddy = &files["configs/Caddyfile"];
    assert!(
        caddy.contains(
            "\ncoord.example.org {\n\ttls /etc/caddy/tls/public.crt /etc/caddy/tls/public.key\n\timport coord\n}\n"
        ),
        "{caddy}"
    );
    assert!(!caddy.contains("email"), "{caddy}");
    // Nothing to obtain, so no ACME state to keep.
    assert!(
        caddy.contains(
            "\nhttp://coord.example.org {\n\tredir https://coord.example.org:7443{uri} 308\n}\n"
        ),
        "plain HTTP is sent to the published port: {caddy}"
    );
    assert!(
        !caddy.contains("\ncoord.example.org:"),
        "a site is its name alone: {caddy}"
    );
    let compose = yaml(&files, "docker-compose.yaml");
    assert!(compose["volumes"].get("caddy_data").is_none());
    assert!(compose["services"].get("ionskale").is_none());
}

#[test]
fn a_lab_one_serves_plain_http_and_takes_insecure_logins() {
    let config = build_coord_config(&options(None, false));
    let files = files_of(&config);
    let caddy = &files["configs/Caddyfile"];
    assert!(
        caddy.contains("\nhttp:// {\n\timport coord\n}\n"),
        "{caddy}"
    );
    assert!(!caddy.contains("https://"), "{caddy}");
    let lok = yaml(&files, "configs/lok.yaml");
    assert_eq!(lok["oidc_issuer"], "http://localhost:7180");
    assert_eq!(lok["django"]["allow_insecure_transport"], true);
    assert_eq!(lok["django"]["secure_cookies"], false);
    let compose = yaml(&files, "docker-compose.yaml");
    assert_eq!(
        compose["services"]["gateway"]["ports"],
        serde_norway::from_str::<Value>("['7180:80']").unwrap()
    );
}

#[test]
fn everything_comes_back_on_its_own_and_keeps_its_logs_small() {
    let files = files_of(&fixture());
    let compose = yaml(&files, "docker-compose.yaml");
    let services = compose["services"].as_mapping().unwrap();
    for (name, service) in services {
        let name = name.as_str().unwrap();
        let expected = if name == "rustfs_init" {
            "on-failure"
        } else {
            "unless-stopped"
        };
        assert_eq!(service["restart"], expected, "{name}");
        assert!(service.get("deploy").is_none(), "{name}");
        assert_eq!(service["logging"]["options"]["max-size"], "10m", "{name}");
    }
    for name in [
        "lok",
        "kontrol",
        "ionskale",
        "lok-reconciler",
        "gateway",
        "db",
    ] {
        assert!(services.contains_key(name), "{name} runs");
    }
}

#[test]
fn the_stack_reaches_its_own_names_at_its_gateway() {
    let config = fixture();
    let compose = yaml(&files_of(&config), "docker-compose.yaml");
    let gateway = &compose["services"]["gateway"];
    let aliases = &gateway["networks"]["default"]["aliases"];
    assert_eq!(
        aliases,
        &serde_norway::from_str::<Value>("[coord.example.org, mesh.coord.example.org]").unwrap()
    );
    assert_eq!(
        gateway["ports"],
        serde_norway::from_str::<Value>("['80:80', '443:443']").unwrap()
    );
    assert!(gateway["volumes"]
        .as_sequence()
        .unwrap()
        .contains(&Value::from("caddy_data:/data")));
    assert!(compose["volumes"].get("caddy_data").is_some());
    let mesh = &compose["services"]["ionskale"];
    assert_eq!(
        mesh["ports"],
        serde_norway::from_str::<Value>("['3478:3478/udp']").unwrap()
    );
    let databases = compose["services"]["db"]["environment"]["POSTGRES_MULTIPLE_DATABASES"]
        .as_str()
        .unwrap();
    assert!(databases.contains("lok_main") && databases.contains("ionskale_main"));
    assert!(config
        .provisioned_databases()
        .contains(&"ionskale_main".to_string()));
}

#[test]
fn the_reconciler_runs_only_what_loks_image_names() {
    let config = fixture();
    let compose = yaml(&files_of(&config), "docker-compose.yaml");
    let reconciler = &compose["services"]["lok-reconciler"];
    let script = reconciler["entrypoint"][2].as_str().unwrap();
    assert_eq!(
        script,
        "while :; do sleep 900; arkitekt-service run reconcile_meshes --sidecars-only || true; done"
    );
    assert_eq!(reconciler["image"], compose["services"]["lok"]["image"]);

    // An image that does not name the job gets no container running it.
    let silent = generate_hub_files(&config, &Default::default(), &Default::default());
    let compose: Value = serde_norway::from_str(&silent["docker-compose.yaml"]).unwrap();
    assert!(compose["services"].get("lok-reconciler").is_none());
}

#[test]
fn lok_and_its_frontend_move_with_update_and_the_rest_waits_for_infra() {
    use konstruktor_core::updates::is_infrastructure;
    let config = fixture();
    assert!(!is_infrastructure(&config, "lok"));
    assert!(!is_infrastructure(&config, "kontrol"));
    for held in ["ionskale", "db", "gateway", "redis"] {
        assert!(is_infrastructure(&config, held), "{held}");
    }
    // A self-contained hub's Lok is infrastructure, as it always was.
    let hub = konstruktor_core::config::hub::build_hub_config(
        &konstruktor_core::config::hub::HubConfigOptions {
            coord_server: "local".into(),
            ..Default::default()
        },
    );
    assert!(is_infrastructure(&hub, "lok"));
}

#[test]
fn a_coordination_server_is_recognised_as_one_on_disk() {
    let dir = std::env::temp_dir().join(format!("konstruktor-coord-{}", rand_suffix()));
    std::fs::create_dir_all(&dir).unwrap();
    let profile = konstruktor_core::profile::hub_profile(fixture());
    konstruktor_core::profile::write_profile(&dir, &profile).unwrap();
    assert_eq!(
        konstruktor_core::profile::holds_a_deployment(&dir),
        Some(konstruktor_core::profile::DeploymentKind::Coord)
    );
    let back = konstruktor_core::profile::read_profile(&dir).unwrap();
    assert!(back.config.is_coord());
    std::fs::remove_dir_all(&dir).ok();
}

fn rand_suffix() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}
