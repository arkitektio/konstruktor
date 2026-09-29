//! An engine attached to a hub whose network does not exist is refused with the reason,
//! not handed to compose to fail on — and once the network is there, it passes the check.

use std::path::PathBuf;

use konstruktor_core::engine::{build_engine_compose, EngineCompose};
use konstruktor_core::engine_probe::EngineKind;
use konstruktor_core::start::missing_external_network;

fn docker(args: &[&str]) -> std::process::Output {
    konstruktor_core::docker::command()
        .args(args)
        .output()
        .expect("docker runs")
}

/// Whether a Docker engine running Linux containers answers — what hubs run on. False, not
/// a panic, when there is no `docker` binary to spawn at all (the macOS CI runners) or the
/// engine runs Windows containers (the Windows runners), where the default bridge network
/// this test creates does not exist.
fn linux_docker_available() -> bool {
    konstruktor_core::docker::command()
        .args(["info", "--format", "{{.OSType}}"])
        .output()
        .is_ok_and(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "linux"
        })
}

#[tokio::test]
async fn an_attached_engine_waits_for_its_hubs_network() {
    if !linux_docker_available() {
        eprintln!("skipping: no Docker engine running Linux containers on this machine");
        return;
    }
    let network = format!("konstruktor-test-hub-net-{}", std::process::id());
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("attached-engine");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let compose = build_engine_compose(&EngineCompose {
        engine: EngineKind::Docker,
        host_socket: None,
        coord_server: "go.arkitekt.live",
        project: "attached-engine",
        hub_network: Some(&network),
        mesh: None,
    });
    std::fs::write(
        dir.join("docker-compose.yaml"),
        serde_norway::to_string(&compose).unwrap(),
    )
    .unwrap();

    assert_eq!(missing_external_network(&dir).await, Some(network.clone()));

    assert!(docker(&["network", "create", &network]).status.success());
    let found = missing_external_network(&dir).await;
    let _ = docker(&["network", "rm", &network]);
    assert_eq!(found, None);

    std::fs::remove_dir_all(&dir).ok();
}
