#!/bin/sh
# Captures the hub folder a released Konstruktor generates into
# crates/konstruktor-core/tests/fixtures/releases/<version>/, for the upgrade tests
# (tests/releases.rs, tests/hub_upgrade.rs). Run once per release that changes what is
# generated:
#
#   scripts/capture-release-fixture.sh 0.14.0
#
# The release's source is exported (not checked out) and asked to generate a default hub.
# The keys and passwords in the result are the throwaway ones of that one generation.
set -eu
version="$1"
root="$(cd "$(dirname "$0")/.." && pwd)"
into="$root/crates/konstruktor-core/tests/fixtures/releases/$version"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

git -C "$root" archive "konstruktor-v$version" | tar -x -C "$work"
cat > "$work/crates/konstruktor-core/tests/capture.rs" <<'RUST'
use konstruktor_core::config::hub::{build_hub_config, HubConfigOptions};
use konstruktor_core::generate::write::write_generated_files;
use konstruktor_core::generate::{generate_hub_files, IssuedIdentity};
use konstruktor_core::profile::{hub_profile, write_profile};

#[test]
fn capture() {
    let dir = std::path::PathBuf::from(std::env::var("CAPTURE_INTO").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    // Fixed ports nothing else is likely to hold: the files name them.
    let config = build_hub_config(&HubConfigOptions {
        device_id: "e2e".into(),
        coord_server: "go.arkitekt.live".into(),
        http_port: Some(18480),
        https_port: Some(18443),
        ..Default::default()
    });
    let files = generate_hub_files(&config, &IssuedIdentity::default());
    write_profile(&dir, &hub_profile(config)).unwrap();
    write_generated_files(&dir, &files).unwrap();
}
RUST
rm -rf "$into"
(cd "$work" && CAPTURE_INTO="$into" CARGO_TARGET_DIR="$root/target/capture" \
    cargo test -p konstruktor-core --test capture)
echo "captured $into"
