//! The half of `deregister` that `tests/deregister.rs` cannot reach: a hub *with* a
//! reporter, whose working login is in a Docker volume.
//!
//! A real `reporter` container and its real volume, against a stand-in coordination
//! server. What it holds still is the promise the module makes — a delete the server
//! refuses leaves the reporter running with the login the server now expects — and that
//! the login asked with is the volume's, not the spent one in `hub_credentials.json`.
//! It needs Docker and the reporter image, so it is opt-in twice over:
//!
//! ```sh
//! KONSTRUKTOR_E2E=1 cargo test -p konstruktor-core --test deregister_volume -- --ignored --nocapture
//! ```
//!
//! The reporter itself never reaches the stand-in — inside its container `127.0.0.1` is
//! not this machine — so it fails its reports quietly and rotates nothing behind the
//! test's back.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use konstruktor_core::config::hub::{build_hub_config, HubConfigOptions, ReporterBlock};
use konstruktor_core::credentials::{credentials_path, CREDENTIALS_FILENAME};
use konstruktor_core::deregister::{deregister, DeregisterError, ServerOutcome, RESCUE_DIR};
use konstruktor_core::profile::{hub_profile, write_profile};
use serde_json::json;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const COMPOSE: &str = r#"services:
  reporter:
    image: jhnnsrs/reporter:latest
    command: [hub-report]
    volumes:
      - ./hub_credentials.json:/seed/hub_credentials.json:ro
      - ./hub_config.yaml:/seed/hub_config.yaml:ro
      - reporter_state:/state
    restart: on-failure
volumes:
  reporter_state: {}
"#;

fn compose(dir: &Path, args: &[&str], stdin: Option<&str>) -> std::process::Output {
    let mut child = konstruktor_core::docker::command()
        .args(["compose"])
        .args(args)
        .current_dir(dir)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("docker runs");
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .expect("stdin was piped")
            .write_all(input.as_bytes())
            .expect("stdin is written");
    }
    child.wait_with_output().expect("docker finishes")
}

fn in_volume(dir: &Path, script: &str, stdin: Option<&str>) -> String {
    let out = compose(
        dir,
        &[
            "run",
            "--rm",
            "--no-deps",
            "-T",
            "--entrypoint",
            "sh",
            "reporter",
            "-c",
            script,
        ],
        stdin,
    );
    assert!(
        out.status.success(),
        "{script}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn token_in_volume(dir: &Path) -> String {
    let state: serde_json::Value =
        serde_json::from_str(&in_volume(dir, "cat /state/reporter.json", None)).expect("a state");
    state["refresh_token"]
        .as_str()
        .expect("a token")
        .to_string()
}

fn reporter_is_running(dir: &Path) -> bool {
    let out = compose(dir, &["ps", "-q", "--status", "running", "reporter"], None);
    !out.stdout.trim_ascii().is_empty()
}

struct Teardown(PathBuf);

impl Drop for Teardown {
    fn drop(&mut self) {
        compose(&self.0, &["down", "--volumes", "--remove-orphans"], None);
        std::fs::remove_dir_all(&self.0).ok();
    }
}

async fn refreshes(server: &MockServer, spent: &str, access: &str, rotated: &str) {
    Mock::given(method("POST"))
        .and(path("/lok/o/token/"))
        .and(body_string_contains(format!("refresh_token={spent}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": access,
            "refresh_token": rotated,
            "expires_in": 3600,
            "token_type": "Bearer"
        })))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "runs a reporter container in Docker; run with KONSTRUKTOR_E2E=1 and --ignored"]
async fn a_refused_delete_leaves_the_reporter_running_with_the_rotated_login() {
    if std::env::var("KONSTRUKTOR_E2E").as_deref() != Ok("1") {
        eprintln!("skipping: set KONSTRUKTOR_E2E=1 to run a reporter");
        return;
    }

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/fakts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token_endpoint": format!("{}/lok/o/token/", server.uri()),
        })))
        .mount(&server)
        .await;

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("deregister-e2e");
    if dir.exists() {
        compose(&dir, &["down", "--volumes", "--remove-orphans"], None);
        std::fs::remove_dir_all(&dir).expect("the old folder is removed");
    }
    std::fs::create_dir_all(&dir).expect("a folder");
    let _teardown = Teardown(dir.clone());

    let mut config = build_hub_config(&HubConfigOptions::default());
    config.reporter = Some(ReporterBlock::default());
    write_profile(&dir, &hub_profile(config)).expect("the profile is written");
    std::fs::write(dir.join("docker-compose.yaml"), COMPOSE).expect("a compose file");
    // The seed: what the hub was authorized with, long since rotated away.
    std::fs::write(
        dir.join(CREDENTIALS_FILENAME),
        json!({
            "version": 1,
            "server": server.uri(),
            "identifier": "mylab",
            "authorizedAt": "2026-01-01T00:00:00Z",
            "envelope": {
                "token_type": "Bearer",
                "access_token": "at-seed",
                "refresh_token": "rt-seed",
                "client_id": "hub-client",
            },
        })
        .to_string(),
    )
    .expect("credentials");

    // The login that works now is the reporter's, in its volume.
    in_volume(
        &dir,
        "cat > /state/reporter.json",
        Some(r#"{"client_id":"hub-client","refresh_token":"rt-live","seeded_from":"hub-client"}"#),
    );
    let up = compose(&dir, &["up", "-d", "reporter"], None);
    assert!(
        up.status.success(),
        "{}",
        String::from_utf8_lossy(&up.stderr)
    );
    assert!(reporter_is_running(&dir));

    // --- the server refuses ---------------------------------------------------------
    refreshes(&server, "rt-live", "at-1", "rt-1").await;
    let refusing = Mock::given(method("POST"))
        .and(path("/lok/f/hubdelete/"))
        .and(header("authorization", "Bearer at-1"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount_as_scoped(&server)
        .await;

    let refused = deregister(&dir).await;
    assert!(
        matches!(refused, Err(DeregisterError::Server { status: 503, .. })),
        "expected a refusal, got {refused:?}"
    );
    drop(refusing);
    assert_eq!(
        token_in_volume(&dir),
        "rt-1",
        "the rotated login is back in the volume"
    );
    assert!(reporter_is_running(&dir), "the reporter was started again");
    assert!(
        !dir.join(RESCUE_DIR).exists(),
        "nothing is left waiting in the folder"
    );
    assert!(
        credentials_path(&dir).exists(),
        "the hub is still registered"
    );

    // --- and then agrees ------------------------------------------------------------
    refreshes(&server, "rt-1", "at-2", "rt-2").await;
    Mock::given(method("POST"))
        .and(path("/lok/f/hubdelete/"))
        .and(header("authorization", "Bearer at-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "deleted" })))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(
        deregister(&dir).await.expect("the server deletes the hub"),
        ServerOutcome::Removed
    );
    assert!(!credentials_path(&dir).exists());
    assert!(
        !reporter_is_running(&dir),
        "a removed hub has nothing to report"
    );
}
