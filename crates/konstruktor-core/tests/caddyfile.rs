use std::path::PathBuf;

use konstruktor_core::catalog::SERVICE_IDS;
use konstruktor_core::config::hub::ServiceBlock;
use konstruktor_core::generate::caddy::{
    build_caddyfile, AgentUpstream, CaddyService, GatewaySites,
};
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

/// Reads the services a parsed profile runs, in whatever order; the emitter re-orders them
/// by `HUB_SERVICE_ORDER` itself, which is part of what is under test.
///
/// Runs, not merely enables: the fixtures are upstream's, which switch Lovekit on without
/// an image — a block that never ran anything (see `ServiceBlock::runs`).
fn services_of(config: &Value) -> Vec<CaddyService<'_>> {
    SERVICE_IDS
        .iter()
        .filter(|id| {
            let block = config.get(id.as_str());
            block
                .and_then(|s| s.get("enabled"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
                && block
                    .and_then(|s| s.get("image"))
                    .is_some_and(|image| !image.is_null())
        })
        .map(|&id| {
            let block = &config[id.as_str()];
            CaddyService {
                id,
                host: str_at(config, id.as_str(), "host"),
                internal_port: block["internal_port"].as_u64().expect("a port") as u16,
                // Through the real lookup, so a bucket the fixture predates gets the same
                // `<service><purpose>` fallback the generator gives an older hub.
                buckets: serde_norway::from_value::<ServiceBlock>(block.clone())
                    .expect("a service block")
                    .bucket_names(id)
                    .into_iter()
                    .map(|(_, name)| name)
                    .collect(),
                // As the generator decides it: takt serves the agent endpoints of a Rekuest
                // this hub runs itself.
                agent_upstream: (id == konstruktor_core::catalog::ServiceId::Rekuest
                    && block.get("image").is_some_and(|image| !image.is_null()))
                .then(|| AgentUpstream {
                    host: format!("{}-takt", str_at(config, id.as_str(), "host")),
                    port: konstruktor_core::config::hub::TAKT_INTERNAL_PORT,
                }),
            }
        })
        .collect()
}

fn caddyfile_for(fixture: &str) -> String {
    let config = config_of(fixture);
    let services = services_of(&config);
    let minio_host = str_at(&config, "minio", "host").to_string();
    let minio_port = config["minio"]["internal_port"].as_u64().expect("a port") as u16;
    // The golden hubs run no Lovekit, so the gateway serves no site beyond their own.
    build_caddyfile(
        &services,
        None,
        &minio_host,
        minio_port,
        &GatewaySites::default(),
    )
}

#[track_caller]
fn assert_matches_golden(fixture: &str, golden: &str) {
    let generated = caddyfile_for(fixture);
    let expected = std::fs::read_to_string(fixtures().join(golden)).expect("golden is readable");

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
