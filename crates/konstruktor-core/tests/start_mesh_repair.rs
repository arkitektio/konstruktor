//! The breakage `start` repairs, produced on purpose: a mesh sidecar that lost one of its
//! networks. Created, never started — the sidecar would need a tailnet to log in to.
//!
//! Skipped when there is no `docker compose`, or when the tailscale image is not already
//! on this machine: pulling it is not something a test run should do behind your back.

use std::path::PathBuf;

use konstruktor_core::config::hub::{build_hub_config, HubConfigOptions};
use konstruktor_core::config::mesh::{MeshOptions, MESH_IMAGE};
use konstruktor_core::generate::write::write_generated_files;
use konstruktor_core::generate::{generate_hub_files, IssuedIdentity};
use konstruktor_core::profile::{hub_profile, write_profile};
use konstruktor_core::start::sidecar_missing_a_network;

fn docker(args: &[&str], dir: &PathBuf) -> std::process::Output {
    konstruktor_core::docker::command()
        .args(args)
        .current_dir(dir)
        .output()
        .expect("docker runs")
}

#[tokio::test]
async fn notices_a_sidecar_that_lost_a_network() {
    let here = std::env::temp_dir();
    let available = konstruktor_core::docker::command()
        .args(["compose", "version"])
        .output()
        .is_ok_and(|o| o.status.success())
        && docker(&["image", "inspect", MESH_IMAGE], &here).status.success();
    if !available {
        eprintln!("skipping: no docker compose, or no {MESH_IMAGE} on this machine");
        return;
    }

    let config = build_hub_config(&HubConfigOptions {
        device_id: "device".into(),
        coord_server: "go.arkitekt.live".into(),
        mesh: Some(MeshOptions {
            hostname: "repair-test".into(),
            auth_key: "tskey-auth-EXAMPLE".into(),
            coord_url: None,
            login: None,
        }),
        ..Default::default()
    });
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mesh-repair");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    write_profile(&dir, &hub_profile(config.clone())).unwrap();
    write_generated_files(&dir, &generate_hub_files(&config, &IssuedIdentity::default())).unwrap();

    struct Down(PathBuf);
    impl Drop for Down {
        fn drop(&mut self) {
            let _ = konstruktor_core::docker::command()
                .args(["compose", "down", "--volumes", "--remove-orphans"])
                .current_dir(&self.0)
                .output();
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _down = Down(dir.clone());

    let created = docker(&["compose", "create", "--no-build", "tailscale"], &dir);
    assert!(
        created.status.success(),
        "compose create failed: {}",
        String::from_utf8_lossy(&created.stderr)
    );

    // Freshly created, it has every network it declares.
    assert_eq!(sidecar_missing_a_network(&dir, &config).await, None);

    // Take one away, as an aborted `up` left it for us.
    let id = String::from_utf8_lossy(&docker(&["compose", "ps", "--all", "--quiet", "tailscale"], &dir).stdout)
        .trim()
        .to_string();
    let disconnected = docker(&["network", "disconnect", &config.internal_network, &id], &dir);
    assert!(
        disconnected.status.success(),
        "network disconnect failed: {}",
        String::from_utf8_lossy(&disconnected.stderr)
    );

    assert_eq!(
        sidecar_missing_a_network(&dir, &config).await.as_deref(),
        Some("attached to 1 of 2")
    );
}
