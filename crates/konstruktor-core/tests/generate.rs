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
