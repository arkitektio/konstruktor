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

struct Case {
    generated: GeneratedFiles,
    expected: GeneratedFiles,
}

fn case(fixture: &str, golden: &str) -> Case {
    Case {
        generated: generate_hub_files(&config_of(fixture), &IssuedIdentity::default()),
        expected: golden_of(golden),
    }
}

fn check(case: &Case) {
    let ours: Vec<&String> = case.generated.keys().collect();
    let theirs: Vec<&String> = case.expected.keys().collect();
    assert_eq!(ours, theirs, "different set of generated files");

    for (name, expected) in &case.expected {
        let generated = &case.generated[name];

        if name.ends_with(".yaml") {
            let ours: Value = serde_norway::from_str(generated)
                .unwrap_or_else(|e| panic!("our {name} is not valid YAML: {e}"));
            let theirs: Value = serde_norway::from_str(expected).expect("golden parses");
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
    const JWKS: &str = "https://go.arkitekt.live/lok/.well-known/jwks.json";

    fn issued() -> IssuedIdentity {
        IssuedIdentity {
            issuer: Some(ISSUER.into()),
            jwks_url: Some(JWKS.into()),
        }
    }

    #[test]
    fn every_service_trusts_the_issuer_the_server_declared() {
        let files = generate_hub_files(&config_of("hub_config.yaml"), &issued());

        for (name, contents) in &files {
            if !name.starts_with("configs/") || !name.ends_with(".yaml") {
                continue;
            }
            let parsed: Value = serde_norway::from_str(contents).expect("valid YAML");
            let Some(authentikate) = parsed.get("authentikate") else {
                continue; // minio_init has none
            };
            let issuers = authentikate["issuers"].as_sequence().expect("a list");
            assert_eq!(issuers.len(), 1, "{name}");
            assert_eq!(issuers[0]["iss"].as_str(), Some(ISSUER), "{name}");
            assert_eq!(issuers[0]["jwks_uri"].as_str(), Some(JWKS), "{name}");
        }
    }

    /// Provenance is a different question from identity: it stays pointed at the local
    /// Rekuest even when the coordination server vouches for everyone's tokens.
    #[test]
    fn provenance_still_points_at_the_local_rekuest() {
        let files = generate_hub_files(&config_of("hub_config.yaml"), &issued());
        let mikro: Value =
            serde_norway::from_str(&files["configs/mikro.yaml"]).expect("valid YAML");

        let provenance = &mikro["authentikate"]["provenance"]["issuers"];
        assert_eq!(provenance[0]["iss"].as_str(), Some("rekuest"));
        assert_eq!(
            provenance[0]["jwks_uri"].as_str(),
            Some("http://rekuest:80/rekuest/.well-known/jwks.json")
        );
    }

    /// Without a grant, both fall back to the CLI's own derivation from `coord_server`.
    #[test]
    fn falls_back_to_the_bare_host_when_there_is_no_grant() {
        let files = generate_hub_files(&config_of("hub_config.yaml"), &IssuedIdentity::default());
        let mikro: Value =
            serde_norway::from_str(&files["configs/mikro.yaml"]).expect("valid YAML");

        let issuers = &mikro["authentikate"]["issuers"];
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
        }));
        config
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
        let compose = compose(&meshed());
        let env = &compose["services"]["tailscale"]["environment"];

        assert_eq!(env["TS_AUTHKEY"].as_str(), Some("tskey-auth-secret"));
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
        }));
        let compose = compose(&config);
        assert!(compose["services"]["tailscale"]["environment"]
            .get("TS_EXTRA_ARGS")
            .is_none());
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

    /// Nothing in the stack runs without an image, so every service compose writes has to
    /// be accounted for — otherwise a whole container silently drops out of the update
    /// check.
    #[test]
    fn no_compose_service_is_left_unaccounted_for() {
        let config = config_of("hub_config.yaml");
        let reported: Vec<String> = config
            .stack_images()
            .into_iter()
            .map(|(service, _)| service)
            .collect();

        for service in compose_service_names(&config) {
            assert!(
                reported.contains(&service),
                "the compose file writes {service}, but stack_images does not report it"
            );
        }
    }
}

/// The dev hub, which also postdates the goldens.
///
/// `mount_github` was ported from upstream's config model and read by nothing until the
/// dev hub existed; these pin the only place it has an effect, from the option a front
/// end sets through to the compose file.
mod dev_hub {
    use super::*;
    use konstruktor_core::catalog::{ServiceId, SERVICE_IDS};
    use konstruktor_core::config::hub::{build_hub_config, HubConfigOptions};

    fn built(dev_hub: bool) -> HubConfig {
        build_hub_config(&HubConfigOptions {
            coord_server: "go.arkitekt.live".into(),
            services: Some(vec![ServiceId::Rekuest, ServiceId::Mikro]),
            dev_hub,
            ..Default::default()
        })
    }

    fn compose(config: &HubConfig) -> Value {
        let files = generate_hub_files(config, &IssuedIdentity::default());
        serde_norway::from_str(&files["docker-compose.yaml"]).expect("valid YAML")
    }

    fn volumes_of(compose: &Value, service: &str) -> Vec<String> {
        compose["services"][service]["volumes"]
            .as_sequence()
            .expect("every service declares volumes")
            .iter()
            .map(|v| v.as_str().expect("a volume is a string").to_string())
            .collect()
    }

    #[test]
    fn an_ordinary_hub_mounts_only_the_config() {
        let plain = built(false);
        assert!(plain
            .enabled_services()
            .iter()
            .all(|id| !plain.service(*id).mount_github));
        assert_eq!(
            volumes_of(&compose(&plain), "rekuest"),
            vec!["./configs/rekuest.yaml:/workspace/config.yaml"]
        );
    }

    #[test]
    fn a_dev_hub_mounts_the_checkout_under_the_config() {
        let config = built(true);
        let dev = compose(&config);

        // The source first and the config second: the config lives *inside* the workspace
        // the checkout provides, and reading them the other way round invites the wrong
        // conclusion about which one survives.
        assert_eq!(
            volumes_of(&dev, "rekuest"),
            vec![
                "./mounts/rekuest:/workspace",
                "./configs/rekuest.yaml:/workspace/config.yaml",
            ]
        );
        assert_eq!(
            volumes_of(&dev, "mikro"),
            vec![
                "./mounts/mikro:/workspace",
                "./configs/mikro.yaml:/workspace/config.yaml",
            ]
        );

        // Only the services run from source; infrastructure is untouched.
        assert!(volumes_of(&dev, "db")
            .iter()
            .all(|v| !v.contains("/mounts/")));
    }

    #[test]
    fn every_service_names_a_repository_to_check_out() {
        let config = built(true);
        for id in SERVICE_IDS {
            let repo = &config.service(id).github_repo;
            assert!(
                repo.starts_with("https://github.com/"),
                "{id:?} has no repository to clone: {repo}"
            );
        }
    }
}

/// The two places the generated stack deliberately goes beyond the Python CLI.
///
/// Both are opt-in, and that is the whole safety argument: a hub nobody customized still
/// generates byte-for-byte what upstream generates, which is what the golden cases above
/// assert. These pin the other half — that asking actually changes something.
mod beyond_upstream {
    use super::*;
    use konstruktor_core::catalog::ServiceId;
    use konstruktor_core::config::hub::{
        build_hub_config, HubConfigOptions, OllamaChoice, ServiceOptions,
    };
    use std::collections::BTreeMap;

    fn with(options: BTreeMap<ServiceId, ServiceOptions>) -> HubConfig {
        build_hub_config(&HubConfigOptions {
            services: Some(vec![ServiceId::Rekuest, ServiceId::Alpaka, ServiceId::Kabinet]),
            service_options: options,
            ..Default::default()
        })
    }

    fn files(config: &HubConfig) -> GeneratedFiles {
        generate_hub_files(config, &IssuedIdentity::default())
    }

    fn yaml(files: &GeneratedFiles, name: &str) -> Value {
        serde_norway::from_str(&files[name]).expect("valid YAML")
    }

    /// A hub that answered nothing must not gain a container, a volume or a config key.
    #[test]
    fn a_hub_nobody_customized_gains_nothing() {
        let config = with(BTreeMap::new());
        assert!(config.local_ollama.is_none());

        let files = files(&config);
        let compose = yaml(&files, "docker-compose.yaml");
        assert!(
            compose["services"].get("ollama").is_none(),
            "no ollama service without being asked"
        );
        assert!(compose["volumes"].get("ollama_models").is_none());
        assert!(yaml(&files, "configs/alpaka.yaml").get("ollama").is_none());
        assert!(
            yaml(&files, "configs/kabinet.yaml")
                .get("ensured_repositories")
                .is_none(),
            "the seeded default stays out of the generated config, as upstream leaves it"
        );
    }

    #[test]
    fn running_ollama_here_adds_the_container_its_volume_and_the_url() {
        let config = with(BTreeMap::from([(
            ServiceId::Alpaka,
            ServiceOptions {
                ollama: Some(OllamaChoice {
                    run_locally: true,
                    url: None,
                }),
                ..Default::default()
            },
        )]));

        let files = files(&config);
        let compose = yaml(&files, "docker-compose.yaml");
        assert_eq!(compose["services"]["ollama"]["image"], "ollama/ollama:latest");
        // Without the volume every restart re-downloads gigabytes of models.
        assert_eq!(
            compose["services"]["ollama"]["volumes"][0],
            "ollama_models:/root/.ollama"
        );
        assert!(compose["volumes"].get("ollama_models").is_some());

        assert_eq!(
            yaml(&files, "configs/alpaka.yaml")["ollama"]["url"],
            "http://ollama:11434"
        );
        assert_eq!(config.alpaka.ollama_config.as_ref().unwrap().kind, "local");
    }

    #[test]
    fn pointing_at_an_ollama_elsewhere_adds_no_container() {
        let config = with(BTreeMap::from([(
            ServiceId::Alpaka,
            ServiceOptions {
                ollama: Some(OllamaChoice {
                    run_locally: false,
                    url: Some("gpu-box.lab:11434".into()),
                }),
                ..Default::default()
            },
        )]));

        let files = files(&config);
        assert!(yaml(&files, "docker-compose.yaml")["services"]
            .get("ollama")
            .is_none());
        // A bare host is plain HTTP, which is what an Ollama on the next machine is.
        assert_eq!(
            yaml(&files, "configs/alpaka.yaml")["ollama"]["url"],
            "http://gpu-box.lab:11434"
        );
        assert_eq!(config.alpaka.ollama_config.as_ref().unwrap().kind, "global");
    }

    /// Alpaka not being in the hub is the case where pulling several gigabytes for a
    /// service that does not exist would be worst.
    #[test]
    fn no_ollama_for_a_hub_without_alpaka() {
        let config = build_hub_config(&HubConfigOptions {
            services: Some(vec![ServiceId::Rekuest]),
            service_options: BTreeMap::from([(
                ServiceId::Alpaka,
                ServiceOptions {
                    ollama: Some(OllamaChoice {
                        run_locally: true,
                        url: None,
                    }),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        });
        assert!(config.local_ollama.is_none());
    }

    #[test]
    fn a_customized_repository_list_reaches_kabinet() {
        let config = with(BTreeMap::from([(
            ServiceId::Kabinet,
            ServiceOptions {
                repositories: Some(vec!["myinstitute/apps:main".into()]),
                ..Default::default()
            },
        )]));

        assert_eq!(
            config.kabinet.ensured_repositories.as_deref(),
            Some(["myinstitute/apps:main".to_string()].as_slice()),
            "an answer replaces the seeded pair rather than adding to it"
        );
        assert_eq!(
            yaml(&files(&config), "configs/kabinet.yaml")["ensured_repositories"][0],
            "myinstitute/apps:main"
        );
    }

    /// The dashboard reconciles the compose file against `stack_images`, so a container
    /// the generator emits but that list forgets shows up as unaccounted for.
    #[test]
    fn the_ollama_container_is_accounted_for_like_every_other() {
        let config = with(BTreeMap::from([(
            ServiceId::Alpaka,
            ServiceOptions {
                ollama: Some(OllamaChoice {
                    run_locally: true,
                    url: None,
                }),
                ..Default::default()
            },
        )]));

        let compose = yaml(&files(&config), "docker-compose.yaml");
        let emitted: Vec<String> = compose["services"]
            .as_mapping()
            .expect("a services mapping")
            .keys()
            .map(|k| k.as_str().expect("a name").to_string())
            .collect();
        let reported: Vec<String> = config
            .stack_images()
            .into_iter()
            .map(|(host, _)| host)
            .collect();

        assert!(emitted.contains(&"ollama".to_string()));
        assert!(
            reported.contains(&"ollama".to_string()),
            "stack_images must know about it: {reported:?}"
        );
    }

    /// `debug` is the one new setting that needed no generator work: it was already being
    /// written, and only the question was missing.
    #[test]
    fn debug_reaches_the_service_config() {
        let config = with(BTreeMap::from([(
            ServiceId::Kabinet,
            ServiceOptions {
                debug: true,
                ..Default::default()
            },
        )]));

        let files = files(&config);
        assert_eq!(yaml(&files, "configs/kabinet.yaml")["django"]["debug"], true);
        assert_eq!(
            yaml(&files, "configs/rekuest.yaml")["django"]["debug"],
            false,
            "it is per service, not per hub"
        );
    }
}
