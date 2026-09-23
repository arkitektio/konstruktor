use std::path::PathBuf;

use konstruktor_core::config::hub::{build_hub_config, HubConfigOptions, ReporterBlock};
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
    konstruktor_core::docker::command()
        .args(["compose", "version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn write_and_validate(mesh: Option<MeshOptions>, label: &str) {
    write_and_validate_with(mesh, false, label)
}

fn write_and_validate_with(mesh: Option<MeshOptions>, reporter: bool, label: &str) {
    if !docker_compose_available() {
        eprintln!("skipping {label}: no docker compose on this machine");
        return;
    }

    let mut config = build_hub_config(&HubConfigOptions {
        device_id: "device".into(),
        coord_server: "go.arkitekt.live".into(),
        mesh,
        ..Default::default()
    });
    if reporter {
        config.reporter = Some(ReporterBlock::default());
    }
    let files = generate_hub_files(&config, &IssuedIdentity::default());

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(label);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    write_generated_files(&dir, &files).expect("files are written");

    let output = konstruktor_core::docker::command()
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
            login: None,
        }),
        "meshed",
    );
}

/// An engine on the mesh and attached to a hub: the deployer in the sidecar's namespace,
/// the sidecar on both networks — the combination `network_mode: service:` makes fussy.
#[test]
fn docker_accepts_a_meshed_attached_engine() {
    use konstruktor_core::config::mesh::build_mesh_block;
    use konstruktor_core::engine::{build_engine_compose, EngineCompose};
    use konstruktor_core::engine_probe::EngineKind;

    if !docker_compose_available() {
        eprintln!("skipping the meshed engine: no docker compose on this machine");
        return;
    }
    let mesh = build_mesh_block(&MeshOptions {
        hostname: "my-engine".into(),
        auth_key: "tskey-auth-EXAMPLE".into(),
        coord_url: Some("https://mesh.example.org".into()),
        login: Some("2-9-48".into()),
    });
    let compose = build_engine_compose(&EngineCompose {
        engine: EngineKind::Docker,
        host_socket: None,
        coord_server: "go.arkitekt.live",
        project: "meshed-engine",
        hub_network: Some("young-dream"),
        mesh: Some(&mesh),
    });

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("meshed-engine");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(
        dir.join("docker-compose.yaml"),
        serde_norway::to_string(&compose).unwrap(),
    )
    .unwrap();

    let output = konstruktor_core::docker::command()
        .args(["compose", "config", "-q"])
        .current_dir(&dir)
        .output()
        .expect("docker runs");
    assert!(
        output.status.success(),
        "docker compose rejected the meshed engine:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// An authorized hub on a mesh: the reporter beside the gateway, sharing the sidecar's
/// socket volume. The most moving parts the generator ever writes.
#[test]
fn docker_accepts_a_meshed_project_with_a_reporter() {
    write_and_validate_with(
        Some(MeshOptions {
            hostname: "lab-hub".into(),
            auth_key: "tskey-auth-EXAMPLE".into(),
            coord_url: Some("https://mesh.example.org".into()),
            login: None,
        }),
        true,
        "meshed-reporter",
    );
}
