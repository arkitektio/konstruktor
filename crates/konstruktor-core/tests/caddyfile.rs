use std::path::PathBuf;

use konstruktor_core::catalog::SERVICE_IDS;
use konstruktor_core::generate::caddy::{build_caddyfile, CaddyService};
use serde_norway::Value;

/// The Caddyfile is the one generated file the TypeScript suite compares byte-for-byte
/// (`generate.test.ts:66`), so it is the one file where a clean-room rewrite can silently
/// diverge — its whitespace is asymmetric and a formatter would happily "fix" it.
///
/// This drives the emitter from the same fixture the TS tests use and diffs the bytes.

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The `config:` sub-tree of a profile — the generator's actual input.
fn config_of(name: &str) -> Value {
    let text = std::fs::read_to_string(fixtures().join(name)).expect("fixture is readable");
    let profile: Value = serde_norway::from_str(&text).expect("fixture parses");
    profile["config"].clone()
}

fn str_at<'a>(config: &'a Value, service: &str, key: &str) -> &'a str {
    config[service][key].as_str().unwrap_or_else(|| {
        panic!("{service}.{key} is missing or not a string");
    })
}

/// Reads the enabled services out of a parsed profile, in whatever order; the emitter
/// re-orders them by `HUB_SERVICE_ORDER` itself, which is part of what is under test.
fn services_of(config: &Value) -> Vec<CaddyService<'_>> {
    SERVICE_IDS
        .iter()
        .filter(|id| {
            config
                .get(id.as_str())
                .and_then(|s| s.get("enabled"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .map(|&id| {
            let block = &config[id.as_str()];
            CaddyService {
                id,
                host: str_at(config, id.as_str(), "host"),
                internal_port: block["internal_port"].as_u64().expect("a port") as u16,
                buckets: id
                    .bucket_purposes()
                    .iter()
                    .filter_map(|purpose| {
                        block
                            .get(format!("{purpose}_bucket"))
                            .and_then(|b| b.get("bucket_name"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .collect(),
            }
        })
        .collect()
}

fn caddyfile_for(fixture: &str) -> String {
    let config = config_of(fixture);
    let services = services_of(&config);
    let minio_host = str_at(&config, "minio", "host").to_string();
    let minio_port = config["minio"]["internal_port"].as_u64().expect("a port") as u16;
    build_caddyfile(&services, &minio_host, minio_port)
}

#[track_caller]
fn assert_matches_golden(fixture: &str, golden: &str) {
    let generated = caddyfile_for(fixture);
    let expected =
        std::fs::read_to_string(fixtures().join(golden)).expect("golden is readable");

    if generated != expected {
        // Bytes, not lines: the divergence is likely to be invisible whitespace.
        panic!(
            "Caddyfile differs from {golden}\n--- generated ---\n{:?}\n--- expected ---\n{:?}",
            generated, expected
        );
    }
}

#[test]
fn matches_the_golden_caddyfile_for_a_local_hub() {
    assert_matches_golden("hub_config.yaml", "golden/hub/configs/Caddyfile");
}

/// The remote-rekuest hub routes one service fewer and two more, so it exercises the
/// ordering rather than just the formatting.
#[test]
fn matches_the_golden_caddyfile_for_a_hub_with_remote_rekuest() {
    assert_matches_golden(
        "hub_config_remote.yaml",
        "golden/hub-remote/configs/Caddyfile",
    );
}

/// The trailing space after `{` on handler blocks is the single most fragile byte in the
/// generator. Assert it directly so a whitespace-trimming edit fails here — with an
/// obvious message — rather than in a whole-file diff.
#[test]
fn handler_braces_keep_their_asymmetric_trailing_space() {
    let generated = caddyfile_for("hub_config.yaml");

    assert!(
        generated.contains("\thandle @rekuest { \n"),
        "service handlers must emit `{{ ` with a trailing space"
    );
    assert!(
        generated.contains("\thandle @minio {\n"),
        "the minio catch-all must emit `{{` with no trailing space"
    );
    assert!(generated.ends_with("\t}\n\n}\n"));
}
