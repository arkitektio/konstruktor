//! Spawns a real hub and asks every service whether it works.
//!
//! `compose_validity` proves Docker accepts the generated project; this proves the project
//! actually *runs* — every container stays up, Postgres accepts connections, and each
//! service answers its health check through the gateway. It pulls every image and takes
//! minutes, so it is opt-in twice over: `#[ignore]`, and `KONSTRUKTOR_E2E=1`.
//!
//! ```sh
//! KONSTRUKTOR_E2E=1 cargo test -p konstruktor-core --test hub_health -- --ignored --nocapture
//! ```
//!
//! `KONSTRUKTOR_E2E_SETTLE_SECS` (default 60) is how long the hub gets after `up` before it
//! is asked; `health::check` then waits and retries on its own on top of that.
//!
//! No coordination server is involved: the hub is generated directly, the way
//! `create_hub` does after the authorization, with a default issued identity.
//!
//! The compose project is left to take its name from the directory, as a real hub's does:
//! `health::check` runs its own `compose exec` there without a `-p`, so a pinned project
//! name would have it looking for Postgres in a project that does not exist.

use std::path::{Path, PathBuf};
use std::time::Duration;

use konstruktor_core::config::hub::{build_hub_config, HubConfigOptions};
use konstruktor_core::generate::write::write_generated_files;
use konstruktor_core::generate::{generate_hub_files, IssuedIdentity};
use konstruktor_core::health::{self, ServiceHealth};
use konstruktor_core::profile::{hub_profile, write_profile};

const DEFAULT_SETTLE: Duration = Duration::from_secs(60);

fn docker_compose_available() -> bool {
    konstruktor_core::docker::command()
        .args(["compose", "version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn compose(dir: &Path, args: &[&str]) -> std::process::Output {
    konstruktor_core::docker::command()
        .args(["compose"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("docker runs")
}

/// A port nothing is listening on right now, so the test never collides with a real hub
/// on 7080 on the same machine.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("a free port")
}

fn settle_time() -> Duration {
    std::env::var("KONSTRUKTOR_E2E_SETTLE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_SETTLE)
}

/// Takes the hub down — volumes included — however the test ends, a failed assertion too.
struct Teardown(PathBuf);

impl Drop for Teardown {
    fn drop(&mut self) {
        eprintln!("tearing the hub down…");
        compose(&self.0, &["down", "--volumes", "--remove-orphans"]);
    }
}

fn report(unhealthy: &[&ServiceHealth]) -> String {
    let mut out = String::from("service | state | restarts | http | detail\n");
    for s in unhealthy {
        out.push_str(&format!(
            "{} | {} | {} | {} | {}\n",
            s.service,
            s.container_state.as_deref().unwrap_or("-"),
            s.restarts_seen,
            s.http_status.map_or("-".to_string(), |c| c.to_string()),
            s.detail,
        ));
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a whole hub in Docker; run with KONSTRUKTOR_E2E=1 and --ignored"]
async fn every_service_of_a_fresh_hub_is_healthy() {
    if std::env::var("KONSTRUKTOR_E2E").as_deref() != Ok("1") {
        eprintln!("skipping: set KONSTRUKTOR_E2E=1 to spawn a hub");
        return;
    }
    if !docker_compose_available() {
        eprintln!("skipping: no docker compose on this machine");
        return;
    }

    let config = build_hub_config(&HubConfigOptions {
        device_id: "e2e".into(),
        coord_server: "go.arkitekt.live".into(),
        http_port: Some(free_port()),
        https_port: Some(free_port()),
        ..Default::default()
    });
    let files = generate_hub_files(&config, &IssuedIdentity::default());

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hub-e2e");
    // A run that crashed hard (no drop) may have left its stack behind.
    if dir.exists() {
        compose(&dir, &["down", "--volumes", "--remove-orphans"]);
        std::fs::remove_dir_all(&dir).expect("old hub dir is removed");
    }
    std::fs::create_dir_all(&dir).expect("hub dir");
    write_profile(&dir, &hub_profile(config.clone())).expect("profile is written");
    write_generated_files(&dir, &files).expect("files are written");

    let _teardown = Teardown(dir.clone());
    let up = compose(&dir, &["up", "-d"]);
    assert!(
        up.status.success(),
        "docker compose up failed:\n{}",
        String::from_utf8_lossy(&up.stderr)
    );

    let settle = settle_time();
    eprintln!("hub is up; letting it settle for {}s…", settle.as_secs());
    tokio::time::sleep(settle).await;

    let results = health::check(&dir, &config, &|event| eprintln!("{event:?}"))
        .await
        .expect("the engine answers");

    assert!(!results.is_empty(), "the health check looked at nothing");
    let unhealthy: Vec<&ServiceHealth> = results.iter().filter(|s| !s.healthy).collect();
    if !unhealthy.is_empty() {
        // The logs are what makes a CI failure diagnosable without re-running it.
        for s in &unhealthy {
            let logs = compose(&dir, &["logs", "--no-color", "--tail", "80", &s.service]);
            eprintln!(
                "----- logs: {} -----\n{}{}",
                s.service,
                String::from_utf8_lossy(&logs.stdout),
                String::from_utf8_lossy(&logs.stderr)
            );
        }
        panic!("unhealthy services:\n{}", report(&unhealthy));
    }
}
