//! A self-contained hub, end to end: generated, started, and *used* — an app's redeem
//! token traded for an access token from this machine, and that token accepted by a
//! service.
//!
//! `hub_health` shows a generated hub comes up. It cannot show the hub can be used,
//! because it has no coordination server to mint a token for it. This one runs its own,
//! so it closes that gap — and it is the only check there is that the issuer written into
//! Lok's config and into every service's agrees with what Lok puts into a token, and that
//! the hub can be logged into at every address it answers on.
//!
//! It pulls and starts a whole stack, so it is opt-in twice over: `#[ignore]`, and
//! `KONSTRUKTOR_E2E=1`.
//!
//! ```sh
//! KONSTRUKTOR_E2E=1 cargo test -p konstruktor-core --test hub_local -- --ignored --nocapture
//! ```
//!
//! `KONSTRUKTOR_E2E_READY_SECS` (default 600) is how long the stack gets to answer.
//! `KONSTRUKTOR_E2E_LOK_IMAGE` runs another Lok image than a new hub gets — one built
//! from a checkout, before it is published.

use std::path::{Path, PathBuf};
use std::time::Duration;

use konstruktor_core::catalog::ServiceId;
use konstruktor_core::config::hub::{build_hub_config, HubConfigOptions, LOCAL_COORD_SERVER};
use konstruktor_core::generate::lok::build_access;
use konstruktor_core::generate::write::write_generated_files;
use konstruktor_core::generate::{generate_hub_files, IssuedIdentity};
use konstruktor_core::profile::{hub_profile, write_profile};
use konstruktor_core::ready;

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

/// A port nothing is listening on right now. It goes into every advertised alias before
/// anything boots, which is why it is picked here rather than left for docker to assign.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("a free port")
}

fn ready_timeout() -> Duration {
    std::env::var("KONSTRUKTOR_E2E_READY_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(600))
}

/// Takes the hub down — volumes included — however the test ends, a failed assertion too.
struct Teardown(PathBuf);

impl Drop for Teardown {
    fn drop(&mut self) {
        eprintln!("tearing the hub down…");
        compose(&self.0, &["down", "--volumes", "--remove-orphans"]);
    }
}

fn logs(dir: &Path, service: &str) -> String {
    let output = compose(dir, &["logs", "--tail", "80", service]);
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "spawns a whole hub in Docker; run with KONSTRUKTOR_E2E=1 and --ignored"]
async fn an_app_redeems_a_token_and_a_service_accepts_it() {
    if std::env::var("KONSTRUKTOR_E2E").as_deref() != Ok("1") {
        eprintln!("skipping: set KONSTRUKTOR_E2E=1 to spawn a hub");
        return;
    }
    if !docker_compose_available() {
        eprintln!("skipping: no docker compose on this machine");
        return;
    }

    let port = free_port();
    let mut config = build_hub_config(&HubConfigOptions {
        device_id: "e2e".into(),
        coord_server: LOCAL_COORD_SERVER.into(),
        services: Some(vec![ServiceId::Rekuest, ServiceId::Mikro]),
        http_port: Some(port),
        https_port: None,
        ..Default::default()
    });
    if let Ok(image) = std::env::var("KONSTRUKTOR_E2E_LOK_IMAGE") {
        config.set_service_image("lok", &image);
    }
    config.csrf_trusted_origins = Some(konstruktor_core::config::hub::trusted_origins(
        &config,
        &["localhost".to_string()],
    ));
    let files = generate_hub_files(&config, &IssuedIdentity::default(), &Default::default());

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hub-local-e2e");
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

    // --- every endpoint a client opens answers ---------------------------------
    let endpoints = ready::wait(&config, ready_timeout(), &|_| {})
        .await
        .expect("the hub publishes a port");
    let unready: Vec<String> = endpoints
        .iter()
        .filter(|e| !e.ready)
        .map(|e| format!("{} ({}) → {:?} {:?}", e.name, e.url, e.status, e.detail))
        .collect();
    assert!(
        unready.is_empty(),
        "not everything answered:\n{}\n\nlok:\n{}",
        unready.join("\n"),
        logs(&dir, "lok")
    );

    // --- the access document is enough to get in --------------------------------
    let lok = config.running_lok().expect("a self-contained hub runs lok");
    let access = build_access(&config, lok);
    let base = access["fakts_url"]
        .as_str()
        .expect("a fakts url")
        .to_string();
    let redeem_token = access["redeem_tokens"][0].as_str().expect("a redeem token");

    let client = reqwest::Client::new();
    let well_known: serde_json::Value = client
        .get(format!("{base}/{}", ready::WELL_KNOWN))
        .send()
        .await
        .expect("the well-known answers")
        .json()
        .await
        .expect("the well-known is JSON");
    // Where the old generator failed: `http://lok/...`, a name only the stack resolves.
    let token_endpoint = well_known["token_endpoint"]
        .as_str()
        .expect("a token endpoint");
    assert_eq!(token_endpoint, format!("{base}/lok/o/token/"));
    // A name, the same wherever it is asked — it is what every service matches.
    assert_eq!(well_known["issuer"], "lok");

    // --- and login is offered at whatever address the hub was reached at ---------
    let by_address: serde_json::Value = client
        .get(format!("http://127.0.0.1:{port}/{}", ready::WELL_KNOWN))
        .send()
        .await
        .expect("the well-known answers at 127.0.0.1")
        .json()
        .await
        .expect("the well-known is JSON");
    assert_eq!(
        by_address["token_endpoint"],
        format!("http://127.0.0.1:{port}/lok/o/token/")
    );
    assert_eq!(by_address["issuer"], "lok");

    // From a container on the stack's own network, where the gateway is `gateway` and
    // nothing is published: what a plugin app started beside the hub sees. Asked from
    // inside the gateway's container, which is on that network and has a `wget`.
    let in_network = compose(
        &dir,
        &[
            "exec",
            "-T",
            "gateway",
            "wget",
            "-qO-",
            &format!("http://gateway/{}", ready::WELL_KNOWN),
        ],
    );
    assert!(
        in_network.status.success(),
        "the well-known did not answer inside the network:\n{}",
        String::from_utf8_lossy(&in_network.stderr)
    );
    let in_network: serde_json::Value =
        serde_json::from_slice(&in_network.stdout).expect("the well-known is JSON");
    assert_eq!(in_network["token_endpoint"], "http://gateway/lok/o/token/");
    assert_eq!(in_network["issuer"], "lok");

    // --- an app trades its redeem token for a client ---------------------------
    let manifest = serde_json::json!({
        "identifier": "live.arkitekt.konstruktor.e2e",
        "version": "1.0.0",
        "scopes": ["openid"],
        "requirements": [
            {"key": "rekuest", "service": "live.arkitekt.rekuest"},
            {"key": "datalayer", "service": "live.arkitekt.s3"},
        ],
    });
    let redeemed = client
        .post(token_endpoint)
        .form(&[
            ("grant_type", "urn:fakts:grant-type:redeem"),
            ("redeem_token", redeem_token),
            ("manifest", &manifest.to_string()),
        ])
        .send()
        .await
        .expect("the token endpoint answers");
    let status = redeemed.status();
    let grant: serde_json::Value = redeemed.json().await.expect("the grant is JSON");
    assert!(
        status.is_success(),
        "the redeem was refused ({status}): {grant}\n\n{}",
        logs(&dir, "lok")
    );

    let access_token = grant["access_token"].as_str().expect("an access token");
    for key in ["rekuest", "datalayer"] {
        assert_eq!(grant["statuses"][key], "granted", "{grant}");
        // The first alias of each is the one this machine can open.
        let alias = &grant["instances"][key]["aliases"][0];
        assert_eq!(alias["host"], "localhost", "{grant}");
        assert_eq!(alias["port"], port, "{grant}");
        // Beside it, the gateway by name, marked as only reachable from the stack's
        // own network.
        let docker = &grant["instances"][key]["aliases"][1];
        assert_eq!(docker["host"], "gateway", "{grant}");
        assert_eq!(docker["kind"], "docker", "{grant}");
        // A pinned key would make the client demand a signed health check. See
        // `no_instance_pins_a_key_its_health_check_cannot_answer_for`.
        assert!(
            grant["instances"][key]["challenge_key"].is_null(),
            "{grant}"
        );
    }

    // --- and a service takes the token Lok signed --------------------------------
    let graphql = format!("{base}/rekuest/graphql");
    let query = serde_json::json!({ "query": "{ agents { id } }" });

    let anonymous: serde_json::Value = client
        .post(&graphql)
        .json(&query)
        .send()
        .await
        .expect("rekuest answers")
        .json()
        .await
        .expect("rekuest answers JSON");
    assert!(
        anonymous["errors"].is_array(),
        "rekuest answered without a token, so the next check would prove nothing: {anonymous}"
    );

    let authenticated: serde_json::Value = client
        .post(&graphql)
        .bearer_auth(access_token)
        .json(&query)
        .send()
        .await
        .expect("rekuest answers")
        .json()
        .await
        .expect("rekuest answers JSON");
    assert!(
        authenticated["errors"].is_null() && authenticated["data"]["agents"].is_array(),
        "rekuest refused a token its own coordination server signed: {authenticated}\n\n{}",
        logs(&dir, "rekuest")
    );
}
