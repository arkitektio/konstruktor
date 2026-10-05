use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use konstruktor_core::config::hub::HubConfig;
use konstruktor_core::generate::{generate_hub_files, GeneratedFiles, IssuedIdentity};
use serde_norway::Value;

/// Konstruktor generates the deployment itself, so the only meaningful test is whether it
/// produces what the Python generator produces.
///
/// `fixtures/golden/<name>` was written by running
/// `arkitekt_next.server.diff.write_hub_files` over `fixtures/<name>.yaml`. Regenerate
/// both together whenever upstream's generator changes.
///
/// One deliberate divergence, edited into the golden files by hand: `authentikate.audience`
/// and `authentikate.provenance.audience`. Current authentikate refuses to start without
/// them, and upstream's generator does not write them yet — see `build_authentikate`.
///
/// A second, larger one, also by hand: object storage is RustFS (`rustfs`, `rustfs_init`,
/// `RUSTFS_*` environment) and every image follows `latest`, where upstream still writes
/// MinIO and channel tags. `jhnnsrs/init` 2.0.0 provisions only RustFS.
///
/// A third, by hand as well: the datalayer, as the services read it today. Every block
/// carries `role_arn` and `session_duration_seconds` — without a role the services refuse
/// every upload and download grant; Rekuest gets a block of its own, since its schema
/// serves media uploads; and every service has a bucket for each store its schema mounts
/// a mutation for — mikro `fabriks` and `konnektion`, elektro `parquet` and `bigfile`,
/// kraph `zarr` and `bigfile` — with their bucket entries and Caddy routes. The fixture
/// profiles predate those buckets, so they also exercise the `<service><purpose>` fallback
/// an older hub takes. See `ServiceId::bucket_purposes` and `build_datalayer`.
///
/// YAML is compared as *parsed structures*: PyYAML, the `yaml` npm package and
/// `serde_norway` all render the same data differently (sequence indentation, quote
/// style, block scalars for PEMs), and none of that is meaningful. The Caddyfile is not
/// YAML and is compared byte-for-byte.

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn config_of(name: &str) -> HubConfig {
    let text = std::fs::read_to_string(fixtures().join(name)).expect("fixture is readable");
    let profile: Value = serde_norway::from_str(&text).expect("fixture parses");
    serde_norway::from_value(profile["config"].clone())
        .unwrap_or_else(|e| panic!("{name} does not deserialize into HubConfig: {e}"))
}

/// Walks `golden/<name>` into the same flat `path -> contents` shape `GeneratedFiles` has.
fn golden_of(name: &str) -> GeneratedFiles {
    let root = fixtures().join("golden").join(name);
    let mut out = BTreeMap::new();

    fn walk(dir: &Path, prefix: &str, out: &mut GeneratedFiles) {
        for entry in std::fs::read_dir(dir).expect("golden dir is readable") {
            let entry = entry.expect("a readable entry");
            let name = entry.file_name().to_string_lossy().to_string();
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if entry.file_type().expect("a file type").is_dir() {
                walk(&entry.path(), &relative, out);
            } else {
                out.insert(
                    relative,
                    std::fs::read_to_string(entry.path()).expect("readable"),
                );
            }
        }
    }

    walk(&root, "", &mut out);
    out
}

/// The key where we deliberately part from the Python generator and that the goldens
/// cannot pin: `instance`, whose keys are minted fresh for every profile that has none (the
/// fixtures predate instance keys). It is tested on its own in `tests/instance_keys.rs`.
/// What hangs off it and *is* deterministic — the services' `rekuest_hook` — is in the
/// goldens; Rekuest's `provenance` and `rekuest.services` / `rekuest.hook_agents` are only written with an
/// instance key, so the fixtures produce neither.
fn without_instance_trust(mut value: Value) -> Value {
    if let Value::Mapping(map) = &mut value {
        map.remove("instance");
    }
    value
}

struct Case {
    generated: GeneratedFiles,
    expected: GeneratedFiles,
}

fn case(fixture: &str, golden: &str) -> Case {
    let generated = generate_hub_files(&config_of(fixture), &IssuedIdentity::default());
    if std::env::var(BLESS).as_deref() == Ok("1") {
        bless(golden, &generated);
    }
    Case {
        generated,
        expected: golden_of(golden),
    }
}

/// Set to overwrite `fixtures/golden/<name>` with what the generator writes today, instead
/// of comparing against it. For a deliberate change to the output — then review the diff:
/// everything in it has to be a change you meant.
///
/// ```sh
/// KONSTRUKTOR_BLESS_GOLDEN=1 cargo test -p konstruktor-core --test generate
/// ```
///
/// The files are written as `serde_norway` renders them, which drops the quotes the goldens
/// keep around `user: '0:0'` (a sexagesimal number to a YAML 1.1 reader). Put them back by
/// hand; the comparison itself parses both sides and does not care.
const BLESS: &str = "KONSTRUKTOR_BLESS_GOLDEN";

fn bless(golden: &str, generated: &GeneratedFiles) {
    let root = fixtures().join("golden").join(golden);
    for (name, contents) in generated {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().expect("inside the golden dir")).unwrap();
        std::fs::write(&path, contents).unwrap();
    }
}

fn check(case: &Case) {
    let ours: Vec<&String> = case.generated.keys().collect();
    let theirs: Vec<&String> = case.expected.keys().collect();
    assert_eq!(ours, theirs, "different set of generated files");

    for (name, expected) in &case.expected {
        let generated = &case.generated[name];

        if name.ends_with(".yaml") {
            let ours: Value = without_instance_trust(
                serde_norway::from_str(generated)
                    .unwrap_or_else(|e| panic!("our {name} is not valid YAML: {e}")),
            );
            let theirs: Value =
                without_instance_trust(serde_norway::from_str(expected).expect("golden parses"));
            assert_eq!(ours, theirs, "{name} differs from the CLI's output");
        } else {
            assert_eq!(generated, expected, "{name} is not byte-identical");
        }
    }
}

#[test]
fn a_local_hub_matches_the_python_generator() {
    check(&case("hub_config.yaml", "hub"));
}

#[test]
fn a_hub_with_remote_rekuest_matches_the_python_generator() {
    check(&case("hub_config_remote.yaml", "hub-remote"));
}

/// The goldens only ever exercise the unauthorized branch, where issuer and JWKS are
/// derived from `coord_server`. This is the path a real hub actually takes.
mod authorized {
    use super::*;

    const ISSUER: &str = "https://go.arkitekt.live";
    /// What a grant hands back — Lok's own mount point.
    const GRANTED_JWKS: &str = "https://go.arkitekt.live/lok/.well-known/jwks.json";
    /// Where the keys are actually served: the base route, not behind `/lok/`.
    const JWKS: &str = "https://go.arkitekt.live/.well-known/jwks.json";

    fn issued() -> IssuedIdentity {
        IssuedIdentity {
            issuer: Some(ISSUER.into()),
            jwks_url: Some(GRANTED_JWKS.into()),
            hub_keys_url: None,
        }
    }

    /// How tokens are verified is one of the hub's facts, the same for every service: what
    /// each image writes its own `authentikate` block from.
    use konstruktor_core::catalog::ServiceId;

    fn auth_of(config: &HubConfig, id: ServiceId, issued: &IssuedIdentity) -> Value {
        konstruktor_core::contract::facts(config, id, issued, &Default::default())["hub"]["auth"]
            .clone()
    }

    #[test]
    fn every_service_trusts_the_issuer_the_server_declared() {
        let config = config_of("hub_config.yaml");
        for id in config.enabled_services() {
            let auth = auth_of(&config, id, &issued());
            let issuers = auth["issuers"].as_sequence().expect("a list");
            assert_eq!(issuers.len(), 1, "{id:?}");
            assert_eq!(issuers[0]["iss"].as_str(), Some(ISSUER), "{id:?}");
            // The issuer is used verbatim; the key set is moved to the base route.
            assert_eq!(issuers[0]["jwks_uri"].as_str(), Some(JWKS), "{id:?}");
        }
    }

    /// Provenance is a different question from identity: it stays pointed at the local
    /// Rekuest even when the coordination server vouches for everyone's tokens.
    #[test]
    fn provenance_still_points_at_the_local_rekuest() {
        let auth = auth_of(&config_of("hub_config.yaml"), ServiceId::Mikro, &issued());

        let provenance = &auth["provenance"]["issuers"];
        assert_eq!(provenance[0]["iss"].as_str(), Some("rekuest"));
        assert_eq!(
            provenance[0]["jwks_uri"].as_str(),
            Some("http://rekuest:80/rekuest/.well-known/jwks.json")
        );
    }

    /// Without a grant, both fall back to the CLI's own derivation from `coord_server`.
    #[test]
    fn falls_back_to_the_bare_host_when_there_is_no_grant() {
        let auth = auth_of(
            &config_of("hub_config.yaml"),
            ServiceId::Mikro,
            &IssuedIdentity::default(),
        );

        let issuers = &auth["issuers"];
        assert_eq!(issuers[0]["iss"].as_str(), Some("go.arkitekt.live"));
        assert_eq!(
            issuers[0]["jwks_uri"].as_str(),
            Some("https://go.arkitekt.live/.well-known/jwks.json")
        );
    }
}

/// Also unrepresented in the goldens — the mesh postdates them.
mod mesh {
    use super::*;
    use konstruktor_core::config::mesh::{build_mesh_block, MeshOptions};

    fn meshed() -> HubConfig {
        let mut config = config_of("hub_config.yaml");
        config.mesh = Some(build_mesh_block(&MeshOptions {
            hostname: "lab-hub".into(),
            auth_key: "tskey-auth-secret".into(),
            coord_url: Some("https://mesh.example.org".into()),
            login: None,
        }));
        config
    }

    /// A key minted for a login keeps its node under that login, so another login starts
    /// a fresh node instead of reviving one the server may have revoked with its session.
    /// One from before servers named the login keeps the directory it always had.
    #[test]
    fn keeps_the_node_state_per_login() {
        let legacy = compose(&meshed());
        assert_eq!(
            legacy["services"]["tailscale"]["environment"]["TS_STATE_DIR"].as_str(),
            Some("/var/lib/tailscale")
        );

        let mut keyed = meshed();
        keyed.mesh.as_mut().unwrap().login = Some("2-3-50".into());
        let keyed = compose(&keyed);
        assert_eq!(
            keyed["services"]["tailscale"]["environment"]["TS_STATE_DIR"].as_str(),
            Some("/var/lib/tailscale/2-3-50")
        );
        // Same volume: the logins sit side by side in it.
        assert_eq!(
            keyed["services"]["tailscale"]["volumes"][0].as_str(),
            Some("tailscale_state:/var/lib/tailscale")
        );
    }

    fn compose(config: &HubConfig) -> Value {
        let files = generate_hub_files(config, &IssuedIdentity::default());
        serde_norway::from_str(&files["docker-compose.yaml"]).expect("valid YAML")
    }

    #[test]
    fn is_absent_unless_a_mesh_was_asked_for() {
        let plain = compose(&config_of("hub_config.yaml"));

        assert!(plain["services"].get("tailscale").is_none());
        assert!(plain["volumes"].get("tailscale_state").is_none());
        // The gateway keeps publishing its own ports when nothing shares its namespace.
        assert_eq!(
            plain["services"]["gateway"]["ports"]
                .as_sequence()
                .map(|p| p.len()),
            Some(2)
        );
        assert!(plain["services"]["gateway"].get("network_mode").is_none());
    }

    #[test]
    fn joins_with_the_key_and_control_server_it_was_given() {
        let files = generate_hub_files(&meshed(), &IssuedIdentity::default());
        let compose: Value = serde_norway::from_str(&files["docker-compose.yaml"]).unwrap();
        let env = &compose["services"]["tailscale"]["environment"];

        // The key is in `mesh.env`, which the sidecar reads — never in the compose file.
        assert!(env.get("TS_AUTHKEY").is_none());
        assert!(!files["docker-compose.yaml"].contains("tskey-auth-secret"));
        assert_eq!(files["mesh.env"], "TS_AUTHKEY=tskey-auth-secret\n");
        assert_eq!(
            compose["services"]["tailscale"]["env_file"],
            serde_norway::from_str::<Value>("[mesh.env]").unwrap()
        );
        // The key is single-use: a restart must not present it again.
        assert_eq!(env["TS_AUTH_ONCE"].as_str(), Some("true"));
        assert_eq!(env["TS_HOSTNAME"].as_str(), Some("lab-hub"));
        assert_eq!(
            env["TS_EXTRA_ARGS"].as_str(),
            Some("--login-server=https://mesh.example.org")
        );
        assert_eq!(
            compose["services"]["tailscale"]["cap_add"],
            serde_norway::from_str::<Value>("[net_admin, sys_module]").unwrap()
        );
    }

    /// `network_mode: service:` forbids `ports` and `networks` on the member, so the
    /// gateway's must move to the sidecar or the stack refuses to start.
    #[test]
    fn moves_the_published_ports_onto_the_namespace_owner() {
        let compose = compose(&meshed());

        assert_eq!(
            compose["services"]["tailscale"]["ports"]
                .as_sequence()
                .map(|p| p.len()),
            Some(2)
        );
        assert_eq!(
            compose["services"]["gateway"]["network_mode"].as_str(),
            Some("service:tailscale")
        );
        assert!(compose["services"]["gateway"].get("ports").is_none());
        assert!(compose["services"]["gateway"].get("networks").is_none());
    }

    /// The gateway lives in the sidecar's namespace; when the sidecar restarts, the
    /// gateway has to be restarted with it or Caddy is left in a dead namespace.
    #[test]
    fn the_gateway_restarts_with_the_sidecar() {
        let compose = compose(&meshed());
        let depends = &compose["services"]["gateway"]["depends_on"]["tailscale"];
        assert_eq!(depends["condition"].as_str(), Some("service_started"));
        assert_eq!(depends["restart"].as_bool(), Some(true));
    }

    #[test]
    fn a_hub_without_a_mesh_writes_no_env_file() {
        let files = generate_hub_files(&config_of("hub_config.yaml"), &IssuedIdentity::default());
        assert!(!files.contains_key("mesh.env"));
    }

    #[test]
    fn keeps_the_node_identity_in_a_named_volume() {
        let compose = compose(&meshed());
        assert!(compose["volumes"].get("tailscale_state").is_some());
    }

    /// No control server means Tailscale's own; the key must simply be absent.
    #[test]
    fn omits_the_login_server_when_there_is_none() {
        let mut config = config_of("hub_config.yaml");
        config.mesh = Some(build_mesh_block(&MeshOptions {
            hostname: "lab-hub".into(),
            auth_key: "tskey-auth-secret".into(),
            coord_url: None,
            login: None,
        }));
        let compose = compose(&config);
        assert!(compose["services"]["tailscale"]["environment"]
            .get("TS_EXTRA_ARGS")
            .is_none());
    }

    /// The gateway has no name of its own inside the sidecar's namespace, so the sidecar
    /// carries it — or plugin apps on the hub's network lose the gateway the moment a
    /// hub joins a mesh.
    #[test]
    fn the_sidecar_answers_to_the_gateway_name() {
        let config = meshed();
        let compose = compose(&config);
        let networks = &compose["services"]["tailscale"]["networks"];

        for network in [config.internal_network.as_str(), "default"] {
            assert_eq!(
                networks[network]["aliases"],
                serde_norway::from_str::<Value>("[gateway]").unwrap(),
                "{network}"
            );
        }
    }

    /// An authorized hub reports its own health, from inside the stack. On a mesh it reads
    /// the sidecar's socket for the name the tailnet gave it.
    #[test]
    fn an_authorized_hub_on_a_mesh_runs_a_reporter_that_can_see_the_sidecar() {
        let mut config = meshed();
        config.reporter = Some(konstruktor_core::config::hub::ReporterBlock::default());
        let compose = compose(&config);

        let reporter = &compose["services"]["reporter"];
        assert_eq!(
            reporter["command"],
            serde_norway::from_str::<Value>("[hub-report]").unwrap()
        );
        let mounts: Vec<&str> = reporter["volumes"]
            .as_sequence()
            .expect("volumes")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(
            mounts.contains(&"./hub_credentials.json:/seed/hub_credentials.json:ro"),
            "{mounts:?}"
        );
        assert!(
            mounts.contains(&"./hub_config.yaml:/seed/hub_config.yaml:ro"),
            "{mounts:?}"
        );
        assert!(mounts.contains(&"reporter_state:/state"), "{mounts:?}");
        assert!(
            mounts.contains(&"tailscale_socket:/var/run/tailscale:ro"),
            "{mounts:?}"
        );

        let sidecar = &compose["services"]["tailscale"];
        assert_eq!(
            sidecar["environment"]["TS_SOCKET"].as_str(),
            Some("/var/run/tailscale/tailscaled.sock")
        );
        assert!(compose["volumes"].get("reporter_state").is_some());
        assert!(compose["volumes"].get("tailscale_socket").is_some());
    }

    /// No reporter, no socket to share: a meshed hub that was never authorized is
    /// generated exactly as before.
    #[test]
    fn the_socket_is_only_shared_with_a_reporter() {
        let compose = compose(&meshed());
        assert!(compose["services"].get("reporter").is_none());
        assert!(compose["services"]["tailscale"]["environment"]
            .get("TS_SOCKET")
            .is_none());
        assert!(compose["volumes"].get("tailscale_socket").is_none());
    }

    /// Mesh-only publishes nothing on the host: no ports on the sidecar, none on the
    /// gateway, and no empty `ports:` left behind.
    #[test]
    fn a_mesh_only_hub_publishes_no_ports() {
        let mut config = meshed();
        config.gateway.exposed_http_port = None;
        config.gateway.exposed_https_port = None;
        config.mesh.as_mut().unwrap().mesh_only = true;

        let compose = compose(&config);
        assert!(compose["services"]["tailscale"].get("ports").is_none());
        assert!(compose["services"]["gateway"].get("ports").is_none());
    }
}

/// The dashboard joins the images a profile declares to the containers Docker reports, on
/// the compose service name. That join is silent when it is wrong — an image nothing
/// matches simply reads as "not running" forever — so the two sides are pinned here.
mod stack_images {
    use super::*;
    use konstruktor_core::config::mesh::{build_mesh_block, MeshOptions};

    fn compose_service_names(config: &HubConfig) -> Vec<String> {
        let files = generate_hub_files(config, &IssuedIdentity::default());
        let compose: Value =
            serde_norway::from_str(&files["docker-compose.yaml"]).expect("valid YAML");
        compose["services"]
            .as_mapping()
            .expect("a services mapping")
            .keys()
            .map(|k| k.as_str().expect("a string key").to_string())
            .collect()
    }

    /// Every key `stack_images` reports has to be a service the compose file actually
    /// writes. `db` is the one that catches drift: the block's host is `daten`, and
    /// reporting that would match no container at all.
    #[test]
    fn every_reported_image_names_a_real_compose_service() {
        for fixture in ["hub_config.yaml", "hub_config_remote.yaml"] {
            let config = config_of(fixture);
            let written = compose_service_names(&config);
            for (service, image) in config.stack_images() {
                assert!(
                    written.contains(&service),
                    "{fixture}: stack_images reports {service} ({image}), \
                     but the compose file writes {written:?}"
                );
            }
        }
    }

    /// The mesh sidecar is a container like any other, and it carries its own image.
    #[test]
    fn the_mesh_sidecar_is_reported_when_there_is_one() {
        let mut config = config_of("hub_config.yaml");
        assert!(!config
            .stack_images()
            .iter()
            .any(|(service, _)| service == "tailscale"));

        config.mesh = Some(build_mesh_block(&MeshOptions {
            hostname: "lab-hub".into(),
            auth_key: "tskey-auth-secret".into(),
            coord_url: None,
            login: None,
        }));

        let written = compose_service_names(&config);
        for (service, image) in config.stack_images() {
            assert!(
                written.contains(&service),
                "stack_images reports {service} ({image}), \
                 but the compose file writes {written:?}"
            );
        }
        assert!(config
            .stack_images()
            .iter()
            .any(|(service, _)| service == "tailscale"));
    }

    /// The reporter runs an image like everything else, so updates and rollbacks see it.
    #[test]
    fn the_reporter_is_reported_both_ways() {
        let mut config = config_of("hub_config.yaml");
        config.reporter = Some(konstruktor_core::config::hub::ReporterBlock::default());

        let written = compose_service_names(&config);
        assert!(written.contains(&"reporter".to_string()), "{written:?}");
        assert!(config
            .stack_images()
            .iter()
            .any(|(service, _)| service == "reporter"));

        // And the way back: a pinned or rolled-back reporter image lands in the profile.
        config.set_service_image("reporter", "ghcr.io/arkitektio/konstruktor:0.0.1");
        assert_eq!(
            config.reporter.as_ref().map(|r| r.image.as_str()),
            Some("ghcr.io/arkitektio/konstruktor:0.0.1")
        );
    }

    /// Nothing in the stack runs without an image, so every service compose writes has to
    /// be accounted for — otherwise a whole container silently drops out of the update
    /// check.
    #[test]
    fn no_compose_service_is_left_unaccounted_for() {
        let mut config = config_of("hub_config.yaml");
        config.reporter = Some(konstruktor_core::config::hub::ReporterBlock::default());
        let reported: Vec<String> = config
            .stack_images()
            .into_iter()
            .map(|(service, _)| service)
            .collect();

        // takt has an image of its own — Rekuest's, with `-takt` on the repository — so
        // it is reported, pulled and pinned as itself, and moves with Rekuest.
        let takt = reported
            .iter()
            .position(|service| service == "rekuest-takt")
            .expect("takt is reported");
        assert_eq!(
            config.stack_images()[takt].1,
            konstruktor_core::config::hub::takt_image_for(config.rekuest.image.as_deref().unwrap())
        );
        assert_eq!(
            konstruktor_core::generate::compose::companions(&config, "rekuest"),
            ["rekuest-takt"]
        );
        assert_eq!(
            konstruktor_core::generate::compose::companion_of(&config, "rekuest-takt").as_deref(),
            Some("rekuest")
        );

        for service in compose_service_names(&config) {
            assert!(
                reported.contains(&service),
                "the compose file writes {service}, but stack_images does not report it"
            );
        }
    }

    /// takt's image follows Rekuest's tag until something pins it, and the pin survives.
    #[test]
    fn takt_follows_rekuests_image_until_pinned() {
        use konstruktor_core::config::hub::takt_image_for;
        assert_eq!(
            takt_image_for("jhnnsrs/rekuest:next"),
            "jhnnsrs/rekuest-takt:next"
        );
        assert_eq!(
            takt_image_for("jhnnsrs/rekuest:next@sha256:abc"),
            "jhnnsrs/rekuest-takt:next"
        );
        assert_eq!(takt_image_for("jhnnsrs/rekuest"), "jhnnsrs/rekuest-takt");
        assert_eq!(
            takt_image_for("registry:5000/lab/rekuest:4.1.0"),
            "registry:5000/lab/rekuest-takt:4.1.0"
        );
        assert_eq!(
            takt_image_for("registry:5000/rekuest"),
            "registry:5000/rekuest-takt"
        );

        let mut config = config_of("hub_config.yaml");
        config.rekuest.image = Some("jhnnsrs/rekuest:4.1.0".into());
        assert_eq!(
            config.takt_image().as_deref(),
            Some("jhnnsrs/rekuest-takt:4.1.0")
        );
        assert_eq!(
            config.takt_url().as_deref(),
            Some("http://rekuest-takt:8080/rekuest")
        );

        // A rollback writes the image it ran back, digest and all.
        config.set_service_image("rekuest-takt", "jhnnsrs/rekuest-takt:4.0.0@sha256:old");
        assert_eq!(
            config.takt_image().as_deref(),
            Some("jhnnsrs/rekuest-takt:4.0.0@sha256:old")
        );
        assert_eq!(
            config.rekuest.image.as_deref(),
            Some("jhnnsrs/rekuest:4.1.0")
        );

        // A hub that runs no Rekuest of its own runs no takt.
        let remote = config_of("hub_config_remote.yaml");
        assert_eq!(remote.takt_host(), None);
        assert_eq!(remote.takt_image(), None);
        assert!(!compose_service_names(&remote).contains(&"rekuest-takt".to_string()));
    }

    /// A rollback, an advanced pin, a re-authorization, a service change and a regenerate
    /// all end by writing every generated file. On a hub whose files are of an older
    /// layout that would write for releases its services are not on, so they refuse and
    /// leave the folder as it is; only an update, which moves the services too, rewrites.
    #[test]
    fn nothing_rewrites_a_hub_of_an_older_layout_in_passing() {
        use konstruktor_core::migrate;
        use konstruktor_core::profile::{self, ProfileError};

        let config = config_of("hub_config.yaml");
        let dir =
            std::env::temp_dir().join(format!("konstruktor-in-passing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        profile::write_profile(&dir, &profile::hub_profile(config.clone())).unwrap();
        let before = "services:\n  rekuest:\n    image: jhnnsrs/rekuest:latest\n  rekuest-reaper:\n    image: jhnnsrs/rekuest:latest\n  mikro:\n    image: jhnnsrs/mikro:latest\n";
        std::fs::write(dir.join("docker-compose.yaml"), before).unwrap();
        let profile_before = std::fs::read_to_string(profile::profile_path(&dir)).unwrap();
        assert_eq!(migrate::layout(&dir, &config), 1);

        let moved = [("mikro".to_string(), "jhnnsrs/mikro:9.9.9".to_string())];
        for refused in [
            profile::rewrite_images(&dir, &moved).unwrap_err(),
            profile::regenerate(&dir).unwrap_err(),
        ] {
            assert!(
                matches!(&refused, ProfileError::Layout(why) if why.contains("konstruktor update")),
                "{refused}"
            );
        }
        assert_eq!(
            std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap(),
            before
        );
        assert_eq!(
            std::fs::read_to_string(profile::profile_path(&dir)).unwrap(),
            profile_before
        );
        assert!(!dir.join("configs").exists(), "no config was written");

        // What an update does to the files: they are what this build generates, and the
        // hub says so from then on.
        profile::rewrite(&dir, config.clone(), &[]).unwrap();
        assert_eq!(migrate::layout(&dir, &config), migrate::CURRENT_LAYOUT);
        assert_eq!(migrate::pending(&dir, &config), []);
        assert!(konstruktor_core::compose_file::declares_service(
            &dir,
            "rekuest-takt"
        ));
        assert!(!konstruktor_core::compose_file::declares_service(
            &dir,
            "rekuest-reaper"
        ));
        assert!(std::fs::read_to_string(dir.join("configs/Caddyfile"))
            .unwrap()
            .contains("reverse_proxy rekuest-takt:8080"));

        // On the layout this build generates, the same rewrites go through.
        profile::rewrite_images(&dir, &moved).unwrap();
        profile::regenerate(&dir).unwrap();
        assert_eq!(
            profile::read_profile(&dir)
                .unwrap()
                .config
                .mikro
                .image
                .as_deref(),
            Some("jhnnsrs/mikro:9.9.9")
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
