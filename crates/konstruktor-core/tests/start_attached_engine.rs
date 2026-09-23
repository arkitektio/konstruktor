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

#[tokio::test]
async fn an_attached_engine_waits_for_its_hubs_network() {
    if !docker(&["version"]).status.success() {
        eprintln!("skipping: no docker on this machine");
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
