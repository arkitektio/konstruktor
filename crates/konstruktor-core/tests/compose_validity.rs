use std::path::PathBuf;

use konstruktor_core::config::hub::{build_hub_config, HubConfigOptions};
use konstruktor_core::config::mesh::MeshOptions;
use konstruktor_core::generate::write::write_generated_files;
use konstruktor_core::generate::{generate_hub_files, IssuedIdentity};

/// The golden tests prove the generated project matches the Python CLI's. This proves
/// Docker itself accepts it — in particular that `network_mode: service:tailscale` and
/// the relocated ports are a combination compose will actually start.
///
/// Skipped when there is no `docker compose` on the machine, so it never fails CI on a
/// runner without Docker.
fn docker_compose_available() -> bool {
    std::process::Command::new("docker")
        .args(["compose", "version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn write_and_validate(mesh: Option<MeshOptions>, label: &str) {
    if !docker_compose_available() {
        eprintln!("skipping {label}: no docker compose on this machine");
        return;
    }

    let config = build_hub_config(&HubConfigOptions {
        device_id: "device".into(),
        coord_server: "go.arkitekt.live".into(),
        mesh,
        ..Default::default()
    });
    let files = generate_hub_files(&config, &IssuedIdentity::default());

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(label);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    write_generated_files(&dir, &files).expect("files are written");

    let output = std::process::Command::new("docker")
        .args(["compose", "config", "-q"])
        .current_dir(&dir)
        .output()
        .expect("docker runs");

    assert!(
        output.status.success(),
        "docker compose rejected the generated project ({label}):\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn docker_accepts_a_plain_project() {
    write_and_validate(None, "plain");
}

#[test]
fn docker_accepts_a_meshed_project() {
    write_and_validate(
        Some(MeshOptions {
            hostname: "lab-hub".into(),
            auth_key: "tskey-auth-EXAMPLE".into(),
            coord_url: Some("https://mesh.example.org".into()),
        }),
        "meshed",
    );
}
