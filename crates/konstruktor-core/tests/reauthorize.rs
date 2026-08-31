//! `create::reauthorize` against a stubbed coordination server.
//!
//! `tests/authorize.rs` covers the device-code protocol itself — what `start` and
//! `poll_once` make of each response. This covers the flow built on top of it: what ends
//! up on disk when the person at the browser accepts, and what does not when they decline.
//!
//! That second half is the one worth asserting. The README's promise is that "only once
//! that comes back does anything get written", and `konstruktor authorize` leans on it
//! entirely: a decline has to leave a working hub exactly as it was, because the profile
//! it would have overwritten holds the secrets the running services already trust.
//!
//! `create_hub` is deliberately not tested here — it probes Docker before its first
//! question and cannot run without a daemon, so it would not be hermetic.

use std::path::{Path, PathBuf};

use konstruktor_core::config::hub::{build_hub_config, HubConfigOptions};
use konstruktor_core::connect::authorize::HubAuthorizationError;
use konstruktor_core::connect::manifest::AdvertisedHost;
use konstruktor_core::create::{reauthorize, CreateError, CreateEvent, ReauthorizeAnswers};
use konstruktor_core::hosts::HostCategory;
use konstruktor_core::profile::{self, hub_profile, write_profile};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// `reauthorize` records the regeneration in the registry, which lives in the platform's
/// data directory. Every variable `dirs` consults for it is pointed at a scratch folder
/// before any test runs, so a test never reads or writes the real one.
fn isolate_registry() -> PathBuf {
    use std::sync::OnceLock;
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("konstruktor-reauth-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("a scratch data directory");
        // Linux reads XDG_DATA_HOME then $HOME; macOS reads $HOME; Windows reads APPDATA.
        std::env::set_var("XDG_DATA_HOME", &root);
        std::env::set_var("HOME", &root);
        std::env::set_var("APPDATA", &root);
        root
    })
    .clone()
}

/// A folder holding a plain, never-authorized hub.
fn a_hub() -> PathBuf {
    isolate_registry();
    let dir = std::env::temp_dir().join(format!(
        "konstruktor-hub-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("the hub folder");

    let config = build_hub_config(&HubConfigOptions {
        device_id: "device".into(),
        coord_server: "coord.example.org".into(),
        ..Default::default()
    });
    write_profile(&dir, &hub_profile(config)).expect("a profile");
    dir
}

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{nanos}{:p}", &nanos)
}

fn answers(dir: &Path, server: &MockServer) -> ReauthorizeAnswers {
    ReauthorizeAnswers {
        dir: dir.to_path_buf(),
        coord_server: server.uri(),
        identifier: "lab-hub".into(),
        description: None,
        hosts: vec![AdvertisedHost {
            host: "lab.example.org".into(),
            kind: HostCategory::Fqdn,
        }],
        reachable_hosts: Vec::new(),
        request_auth_key: false,
    }
}

/// A coordination server that stages a grant, and answers the token endpoint with
/// whatever `token` says — which is the only difference between accepting and declining.
async fn coordination_server(token: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/.well-known/fakts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "issuer": "https://coord.example.org",
            "hub_authorization_endpoint": format!("{}/o/hub-authorization/", server.uri()),
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/o/hub-authorization/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "granted",
            "device_code": "kJ8f-full-entropy",
            "user_code": "A7K3",
            "client_id": "9c1d",
            "token_endpoint": format!("{}/o/token/", server.uri()),
            "verification_uri": format!("{}/hubconfigure/", server.uri()),
            "verification_uri_complete": format!("{}/hubconfigure/A7K3", server.uri()),
            "expires_in": 300,
            "interval": 5
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/o/token/"))
        .respond_with(token)
        .mount(&server)
        .await;

    server
}

/// The person at the browser pressed Accept.
fn accepted(extra_auth: serde_json::Value) -> ResponseTemplate {
    let mut auth = json!({ "jwks_url": "https://coord.example.org/.well-known/jwks.json" });
    if let (Some(a), Some(b)) = (auth.as_object_mut(), extra_auth.as_object()) {
        for (k, v) in b {
            a.insert(k.clone(), v.clone());
        }
    }
    ResponseTemplate::new(200).set_body_json(json!({
        "token_type": "Bearer",
        "access_token": "eyJ",
        "client_id": "9c1d",
        "auth": auth,
    }))
}

/// The person at the browser pressed Decline. The endpoint says so with a 400.
fn declined() -> ResponseTemplate {
    ResponseTemplate::new(400).set_body_json(json!({ "error": "access_denied" }))
}

fn collect(events: &std::sync::Mutex<Vec<String>>) -> impl Fn(CreateEvent) + Sync + '_ {
    move |event| {
        let name = match event {
            CreateEvent::CheckingDocker => "checking-docker",
            CreateEvent::Building => "building",
            CreateEvent::Staged { .. } => "staged",
            CreateEvent::Waiting { .. } => "waiting",
            CreateEvent::Granted { .. } => "granted",
            CreateEvent::Writing { .. } => "writing",
            CreateEvent::Cloning { .. } => "cloning",
            CreateEvent::Starting => "starting",
            CreateEvent::Log { .. } => "log",
            CreateEvent::Done { .. } => "done",
        };
        events.lock().expect("the log").push(name.to_string());
    }
}

// --- accepted --------------------------------------------------------------------------

#[tokio::test]
async fn an_accepted_hub_gets_its_credentials_and_regenerated_configs() {
    let dir = a_hub();
    let server = coordination_server(accepted(json!({}))).await;
    let events = std::sync::Mutex::new(Vec::new());

    let credentials = reauthorize(
        &answers(&dir, &server),
        &CancellationToken::new(),
        &collect(&events),
    )
    .await
    .expect("the hub is authorized");

    assert_eq!(credentials.identifier, "lab-hub");
    assert_eq!(
        credentials.envelope.auth.jwks_url.as_deref(),
        Some("https://coord.example.org/.well-known/jwks.json")
    );
    // What the hub told the server it is reachable at is kept, so the next authorization
    // can start from it rather than from a fresh scan of this machine.
    assert_eq!(credentials.advertised_hosts.len(), 1);
    assert_eq!(credentials.advertised_hosts[0].host, "lab.example.org");

    // On disk, not merely returned.
    let written = konstruktor_core::credentials::read_credentials(&dir)
        .expect("the credentials are on disk");
    assert_eq!(written.identifier, "lab-hub");
    assert_eq!(written.issuer.as_deref(), Some("https://coord.example.org"));

    // The service configs are regenerated because the JWKS URL they verify tokens
    // against may have moved.
    assert!(
        dir.join("configs").is_dir(),
        "the service configs were not regenerated"
    );
    assert!(dir.join("docker-compose.yaml").is_file());

    let seen = events.lock().expect("the log").clone();
    for expected in ["building", "staged", "granted", "writing", "done"] {
        assert!(seen.contains(&expected.to_string()), "missing {expected} in {seen:?}");
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// Asking for a mesh key is the reason `authorize` exists a second time: the tailnet
/// address only exists once the hub has joined, and joining needs the key.
#[tokio::test]
async fn a_granted_mesh_key_lands_in_the_profile() {
    let dir = a_hub();
    let server = coordination_server(accepted(json!({
        "ionscale_auth_key": "tskey-auth-minted",
        "ionscale_coord_url": "https://mesh.example.org"
    })))
    .await;

    let mut wanted = answers(&dir, &server);
    wanted.request_auth_key = true;

    reauthorize(&wanted, &CancellationToken::new(), &|_| {})
        .await
        .expect("the hub is authorized");

    let profile = profile::read_profile(&dir).expect("the profile still reads");
    let mesh = profile.config.mesh.expect("a mesh block was written");
    assert!(mesh.enabled);
    assert_eq!(mesh.auth_key, "tskey-auth-minted");
    assert_eq!(mesh.hostname, "lab-hub");
    assert_eq!(mesh.coord_url.as_deref(), Some("https://mesh.example.org"));

    std::fs::remove_dir_all(&dir).ok();
}

/// A key that was not asked for is not folded in, even if the server sends one.
#[tokio::test]
async fn a_mesh_key_that_was_not_requested_is_not_written() {
    let dir = a_hub();
    let server = coordination_server(accepted(json!({
        "ionscale_auth_key": "tskey-auth-unasked-for"
    })))
    .await;

    reauthorize(&answers(&dir, &server), &CancellationToken::new(), &|_| {})
        .await
        .expect("the hub is authorized");

    let profile = profile::read_profile(&dir).expect("the profile still reads");
    assert!(
        profile.config.mesh.is_none(),
        "a mesh block appeared without --request-auth-key"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// --- declined --------------------------------------------------------------------------

/// The whole point of the two-phase flow: a decline leaves the hub exactly as it was.
///
/// Not merely "no credentials" — the profile carries the secrets and the provenance key
/// the running services already trust, so a half-written folder after a refused
/// authorization would be worse than no authorization at all.
#[tokio::test]
async fn a_declined_hub_is_left_completely_untouched() {
    let dir = a_hub();
    let before = std::fs::read(profile::profile_path(&dir)).expect("the profile");
    let listing_before = listing(&dir);

    let server = coordination_server(declined()).await;
    let events = std::sync::Mutex::new(Vec::new());

    let error = reauthorize(
        &answers(&dir, &server),
        &CancellationToken::new(),
        &collect(&events),
    )
    .await
    .expect_err("a declined authorization is an error");

    assert!(
        matches!(
            error,
            CreateError::Authorization(HubAuthorizationError::Declined)
        ),
        "got {error:?}"
    );

    // Nothing new appeared…
    assert_eq!(listing(&dir), listing_before, "the folder gained files");
    assert!(
        !konstruktor_core::credentials::credentials_path(&dir).exists(),
        "credentials were written for a declined authorization"
    );
    assert!(!dir.join("configs").exists());
    assert!(!dir.join("docker-compose.yaml").exists());

    // …and nothing existing changed.
    assert_eq!(
        std::fs::read(profile::profile_path(&dir)).expect("the profile"),
        before,
        "the profile was rewritten for a declined authorization"
    );

    // It got as far as asking, and stopped before writing.
    let seen = events.lock().expect("the log").clone();
    assert!(seen.contains(&"staged".to_string()), "{seen:?}");
    assert!(
        !seen.contains(&"writing".to_string()) && !seen.contains(&"done".to_string()),
        "it reported writing after a decline: {seen:?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// The same guarantee when the grant runs out rather than being refused.
#[tokio::test]
async fn an_expired_authorization_writes_nothing_either() {
    let dir = a_hub();
    let before = listing(&dir);
    let server = coordination_server(
        ResponseTemplate::new(400).set_body_json(json!({ "error": "expired_token" })),
    )
    .await;

    let error = reauthorize(&answers(&dir, &server), &CancellationToken::new(), &|_| {})
        .await
        .expect_err("an expired grant is an error");

    assert!(
        matches!(
            error,
            CreateError::Authorization(HubAuthorizationError::Expired)
        ),
        "got {error:?}"
    );
    assert_eq!(listing(&dir), before);

    std::fs::remove_dir_all(&dir).ok();
}

/// Ctrl-C while waiting for the browser is the third way this ends, and it has to leave
/// the same nothing behind.
#[tokio::test]
async fn a_cancelled_authorization_writes_nothing() {
    let dir = a_hub();
    let before = listing(&dir);
    // Never answers "granted", so the only way out is the cancellation.
    let server = coordination_server(
        ResponseTemplate::new(400).set_body_json(json!({ "error": "authorization_pending" })),
    )
    .await;

    let cancel = CancellationToken::new();
    cancel.cancel();

    let error = reauthorize(&answers(&dir, &server), &cancel, &|_| {})
        .await
        .expect_err("a cancelled authorization is an error");

    assert!(
        matches!(
            error,
            CreateError::Authorization(HubAuthorizationError::Cancelled)
        ),
        "got {error:?}"
    );
    assert_eq!(listing(&dir), before);

    std::fs::remove_dir_all(&dir).ok();
}

/// A refusal at the staging call, before any browser is involved at all — the identifier
/// is already taken, say. Also must write nothing.
#[tokio::test]
async fn a_refused_manifest_writes_nothing() {
    let dir = a_hub();
    let before = listing(&dir);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/fakts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "issuer": "https://coord.example.org",
            "hub_authorization_endpoint": format!("{}/o/hub-authorization/", server.uri()),
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/o/hub-authorization/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "error",
            "error_description": "that identifier is taken"
        })))
        .mount(&server)
        .await;

    let error = reauthorize(&answers(&dir, &server), &CancellationToken::new(), &|_| {})
        .await
        .expect_err("a refused manifest is an error");

    assert!(
        matches!(
            error,
            CreateError::Authorization(HubAuthorizationError::Refused(ref d))
                if d.contains("identifier is taken")
        ),
        "got {error:?}"
    );
    assert_eq!(listing(&dir), before);

    std::fs::remove_dir_all(&dir).ok();
}

/// Every name in the folder, sorted — enough to catch a file appearing where none should.
fn listing(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
}
