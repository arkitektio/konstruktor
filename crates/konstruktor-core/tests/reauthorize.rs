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

use konstruktor_core::catalog::ServiceId;
use konstruktor_core::config::hub::HubConfigOptions;
use konstruktor_core::connect::authorize::HubAuthorizationError;
use konstruktor_core::connect::manifest::AdvertisedHost;
use konstruktor_core::create::{
    reauthorize, CreateError, CreateEvent, MeshKeyRequest, ReauthorizeAnswers,
};
use konstruktor_core::hosts::HostCategory;
use konstruktor_core::profile::{self, hub_profile, write_profile};
use konstruktor_core::services::{answers_from_disk, ServiceChange};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod support;

/// `reauthorize` records the regeneration in the registry, which lives in the platform's
/// data directory. It is pointed at a scratch folder before any test runs, so a test
/// never reads or writes the real one — through the explicit override, because on Windows
/// `dirs` ignores `APPDATA`.
fn isolate_registry() -> PathBuf {
    use std::sync::OnceLock;
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    let root = ROOT
        .get_or_init(|| {
            let root =
                std::env::temp_dir().join(format!("konstruktor-reauth-{}", std::process::id()));
            std::fs::create_dir_all(&root).expect("a scratch data directory");
            std::env::set_var(konstruktor_core::registry::DATA_DIR_ENV, &root);
            root
        })
        .clone();
    let path = konstruktor_core::registry::registry_path().expect("a registry path");
    assert!(
        path.starts_with(&root),
        "refusing to touch a registry outside the scratch folder: {}",
        path.display()
    );
    root
}

/// A folder holding a plain, never-authorized hub, as `hub create` leaves one: its
/// services provided what their images asked for, and what the images said written down.
fn a_hub() -> PathBuf {
    isolate_registry();
    let dir = std::env::temp_dir().join(format!(
        "konstruktor-hub-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("the hub folder");

    let config = support::hub(&HubConfigOptions {
        device_id: "device".into(),
        coord_server: "coord.example.org".into(),
        ..Default::default()
    });
    write_profile(&dir, &hub_profile(config.clone())).expect("a profile");
    konstruktor_core::contract::remember(&dir, &config, &support::said())
        .expect("what the images said");
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
        mesh_key: MeshKeyRequest::Never,
        services: None,
        // What the image of a service that is added says, by image, as if it had just
        // been asked: no test here has an engine to ask one with.
        described: konstruktor_core::catalog::SERVICE_IDS
            .iter()
            .filter_map(|id| {
                Some((
                    id.default_image()?.to_string(),
                    support::description_of(*id),
                ))
            })
            .collect(),
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

/// The person at the browser pressed Accept. The response is the current shape: the JWKS
/// URL and whose login this is under `self`, and `mesh` present only when it has a key.
fn accepted(mesh: serde_json::Value) -> ResponseTemplate {
    let mut body = json!({
        "token_type": "Bearer",
        "access_token": "eyJ",
        "refresh_token": "rt-1",
        "client_id": "9c1d",
        "self": {
            "jwks_url": "https://coord.example.org/.well-known/jwks.json",
            "sub": "2",
            "organization": "3",
            "hub": "50"
        },
    });
    if mesh.as_object().is_some_and(|a| !a.is_empty()) {
        body["mesh"] = mesh;
    }
    ResponseTemplate::new(200).set_body_json(body)
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

    let done = reauthorize(
        &answers(&dir, &server),
        &CancellationToken::new(),
        &collect(&events),
    )
    .await
    .expect("the hub is authorized");
    let credentials = &done.credentials;

    assert_eq!(credentials.identifier, "lab-hub");
    assert_eq!(
        credentials.envelope.jwks_url(),
        Some("https://coord.example.org/.well-known/jwks.json")
    );
    // What the hub told the server it is reachable at is kept, so the next authorization
    // can start from it rather than from a fresh scan of this machine.
    assert_eq!(credentials.advertised_hosts.len(), 1);
    assert_eq!(credentials.advertised_hosts[0].host, "lab.example.org");

    // On disk, not merely returned.
    let written =
        konstruktor_core::credentials::read_credentials(&dir).expect("the credentials are on disk");
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
        assert!(
            seen.contains(&expected.to_string()),
            "missing {expected} in {seen:?}"
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// A hub that is not on a mesh asks for a key by default, and a granted one becomes its
/// mesh block.
#[tokio::test]
async fn a_hub_off_the_mesh_asks_for_a_key_and_keeps_it() {
    let dir = a_hub();
    let server = coordination_server(accepted(json!({
        "ionscale_auth_key": "tskey-auth-minted",
        "ionscale_coord_url": "https://mesh.example.org"
    })))
    .await;

    let mut wanted = answers(&dir, &server);
    wanted.mesh_key = MeshKeyRequest::Auto;

    let done = reauthorize(&wanted, &CancellationToken::new(), &|_| {})
        .await
        .expect("the hub is authorized");
    assert!(done.mesh_requested);
    assert!(done.mesh_granted);
    assert!(done.reporter_enabled);

    let profile = profile::read_profile(&dir).expect("the profile still reads");
    let mesh = profile.config.mesh.expect("a mesh block was written");
    assert!(mesh.enabled);
    assert_eq!(mesh.auth_key, "tskey-auth-minted");
    assert_eq!(mesh.hostname, "lab-hub");
    assert_eq!(mesh.coord_url.as_deref(), Some("https://mesh.example.org"));
    // The key belongs to the login that was granted; the node's state is kept under it.
    assert_eq!(mesh.login.as_deref(), Some("2-3-50"));
    // The grant carried a refresh token, so the hub can report its own health now.
    assert!(
        profile.config.reporter.is_some(),
        "no reporter after authorization"
    );

    // And asked again next time: which login that grant is only shows once accepted.
    let server = coordination_server(accepted(json!({
        "ionscale_auth_key": "tskey-auth-second",
        "ionscale_coord_url": "https://mesh.example.org"
    })))
    .await;
    let mut again = answers(&dir, &server);
    again.mesh_key = MeshKeyRequest::Auto;
    let done = reauthorize(&again, &CancellationToken::new(), &|_| {})
        .await
        .expect("the hub is authorized again");
    assert!(done.mesh_requested);
    let mesh = profile::read_profile(&dir).unwrap().config.mesh.unwrap();
    assert_eq!(mesh.auth_key, "tskey-auth-second");
    assert_eq!(mesh.login.as_deref(), Some("2-3-50"));

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
        "a mesh block appeared although no key was asked for"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// A mesh-only hub publishes no port, so an address on this machine's networks is never
/// advertised for it — whoever passed one. A fresh key keeps it mesh-only.
#[tokio::test]
async fn a_mesh_only_hub_advertises_no_host_and_stays_mesh_only() {
    let dir = a_hub();
    let mut profile = profile::read_profile(&dir).unwrap();
    let mut block = konstruktor_core::config::mesh::build_mesh_block(
        &konstruktor_core::config::mesh::MeshOptions {
            hostname: "lab-hub".into(),
            auth_key: "tskey-auth-old".into(),
            coord_url: None,
            login: None,
        },
    );
    block.mesh_only = true;
    profile.config.mesh = Some(block);
    write_profile(&dir, &profile).unwrap();

    let server =
        coordination_server(accepted(json!({ "ionscale_auth_key": "tskey-auth-fresh" }))).await;
    let mut wanted = answers(&dir, &server);
    wanted.mesh_key = MeshKeyRequest::Fresh;

    let done = reauthorize(&wanted, &CancellationToken::new(), &|_| {})
        .await
        .expect("the hub is authorized");

    assert!(
        done.credentials.advertised_hosts.is_empty(),
        "{:?}",
        done.credentials.advertised_hosts
    );
    let mesh = profile::read_profile(&dir)
        .unwrap()
        .config
        .mesh
        .expect("a mesh block");
    assert_eq!(mesh.auth_key, "tskey-auth-fresh");
    assert!(mesh.mesh_only, "a fresh key must not undo mesh-only");

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

// --- changing services ------------------------------------------------------------------

fn with_change(
    dir: &Path,
    server: &MockServer,
    add: &[ServiceId],
    remove: &[ServiceId],
) -> ReauthorizeAnswers {
    let mut wanted = answers(dir, server);
    wanted.services = Some(ServiceChange {
        add: add.to_vec(),
        remove: remove.to_vec(),
        ..Default::default()
    });
    wanted
}

fn compose(dir: &Path) -> serde_norway::Value {
    serde_norway::from_str(&std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap())
        .expect("the compose file parses")
}

fn compose_services(dir: &Path) -> Vec<String> {
    compose(dir)["services"]
        .as_mapping()
        .expect("services")
        .keys()
        .filter_map(|k| k.as_str().map(str::to_string))
        .collect()
}

fn init_databases(dir: &Path) -> Vec<String> {
    compose(dir)["services"]["db"]["environment"]["POSTGRES_MULTIPLE_DATABASES"]
        .as_str()
        .expect("the init list")
        .split(',')
        .map(str::to_string)
        .collect()
}

fn bucket_manifest(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("configs/rustfs_init.yaml")).expect("the bucket manifest")
}

/// Every secret a running hub's data was written with — none of which a service change
/// may touch. Of the services in `kept`: a disabled experimental block that holds nothing
/// is not written to the profile at all, so it is minted afresh on every read.
fn secrets_of(
    config: &konstruktor_core::config::hub::HubConfig,
    kept: &[ServiceId],
) -> Vec<String> {
    let mut out = vec![
        config.db.postgres_user.clone(),
        config.db.postgres_password.clone(),
        config.minio.access_key.clone(),
        config.minio.secret_key.clone(),
        config.minio.root_user.clone(),
        config.minio.root_password.clone(),
        config.global_admin_password.clone(),
    ];
    for id in kept.iter().copied() {
        let block = config.service(id);
        out.push(format!("{id:?} secret {}", block.secret_key));
        out.push(format!(
            "{id:?} key {:?}",
            block
                .instance_key_pair
                .as_ref()
                .map(|k| k.private_key.clone())
        ));
    }
    out
}

/// Adding Bank to a hub: its container, config and bucket appear, and every key and
/// secret the running services hold is exactly what it was.
#[tokio::test]
async fn adding_bank_emits_it_and_keeps_every_existing_secret() {
    let dir = a_hub();
    let before = profile::read_profile(&dir).unwrap().config;
    assert!(
        !before
            .service(konstruktor_core::catalog::ServiceId::Bank)
            .enabled
    );

    let server = coordination_server(accepted(json!({}))).await;
    let done = reauthorize(
        &with_change(&dir, &server, &[ServiceId::Bank], &[]),
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .expect("the change is accepted");
    let plan = done.services.expect("a plan");
    assert_eq!(plan.added, [ServiceId::Bank]);
    assert!(plan.removed.is_empty());

    let after = profile::read_profile(&dir).unwrap().config;
    assert!(
        after
            .service(konstruktor_core::catalog::ServiceId::Bank)
            .enabled
    );
    assert!(
        after
            .service(konstruktor_core::catalog::ServiceId::Bank)
            .instance_key_pair
            .is_some(),
        "bank got no instance key"
    );

    // Nothing that already ran moved; Bank's own block is the one seeded at creation.
    let kept: Vec<ServiceId> = before.enabled_services();
    assert!(kept.contains(&ServiceId::Mikro) && !kept.contains(&ServiceId::Bank));
    assert_eq!(secrets_of(&before, &kept), secrets_of(&after, &kept));

    assert!(compose_services(&dir).contains(&"bank".to_string()));
    // Rekuest is told of it — its own image writes the hook agent into its config from
    // that — and the gateway routes it.
    let told = konstruktor_core::contract::facts(
        &after,
        ServiceId::Rekuest,
        &Default::default(),
        &Default::default(),
    );
    assert_eq!(
        told["peers"]["bank"]["url"].as_str(),
        Some("http://bank:80/bank"),
        "rekuest is not told of bank"
    );
    let caddyfile = std::fs::read_to_string(dir.join("configs/Caddyfile")).unwrap();
    assert!(caddyfile.contains("/bank"), "bank is not routed");
    assert!(init_databases(&dir).contains(&"bank".to_string()));
    assert!(bucket_manifest(&dir).contains("bankbigfile"));

    // The manifest carried the new instance, with its key, to the coordination server.
    let sent = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.url.path() == "/o/hub-authorization/")
        .expect("the manifest was sent");
    let body = String::from_utf8_lossy(&sent.body).to_string();
    assert!(
        body.contains("live.arkitekt.bank"),
        "bank is not in the manifest: {body}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Taking Kraph out drops its container and route, but its database stays in the init
/// list and its buckets in the manifest: the data is kept, not orphaned.
#[tokio::test]
async fn removing_a_service_drops_its_container_but_keeps_its_data() {
    let dir = a_hub();
    let server = coordination_server(accepted(json!({}))).await;
    reauthorize(
        &with_change(&dir, &server, &[], &[ServiceId::Kraph]),
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .expect("the change is accepted");

    let services = compose_services(&dir);
    assert!(!services.contains(&"kraph".to_string()), "{services:?}");
    assert!(services.contains(&"mikro".to_string()));
    assert!(init_databases(&dir).contains(&"kraph".to_string()));
    assert!(bucket_manifest(&dir).contains("kraphzarr"));
    let caddyfile = std::fs::read_to_string(dir.join("configs/Caddyfile")).unwrap();
    assert!(!caddyfile.contains("/kraph"), "kraph is still routed");

    let config = profile::read_profile(&dir).unwrap().config;
    assert!(
        !config
            .service(konstruktor_core::catalog::ServiceId::Kraph)
            .enabled
    );
    assert!(
        config
            .service(konstruktor_core::catalog::ServiceId::Kraph)
            .retained
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Rekuest runs the hooked services' periodic work: it cannot go while one of them runs,
/// and the refusal comes before any request leaves this machine.
#[tokio::test]
async fn rekuest_cannot_be_removed_while_hooked_services_run() {
    let dir = a_hub();
    let before = listing(&dir);
    let profile_before = std::fs::read(profile::profile_path(&dir)).unwrap();
    let server = coordination_server(accepted(json!({}))).await;

    let error = reauthorize(
        &with_change(&dir, &server, &[], &[ServiceId::Rekuest]),
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .expect_err("refused");
    assert!(
        matches!(error, CreateError::Answers(ref m) if m.contains("keep Rekuest")),
        "{error:?}"
    );
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "a request was sent"
    );
    assert_eq!(listing(&dir), before);
    assert_eq!(
        std::fs::read(profile::profile_path(&dir)).unwrap(),
        profile_before
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Kuvert's Fernet key encrypts every linked mailbox's credentials: taking Kuvert out and
/// adding it back has to find the same key, or those mailboxes are unreadable.
#[tokio::test]
async fn re_adding_kuvert_keeps_its_fernet_key() {
    let dir = a_hub();
    let server = coordination_server(accepted(json!({}))).await;
    let run = |add: &'static [ServiceId], remove: &'static [ServiceId]| {
        let answers = with_change(&dir, &server, add, remove);
        async move {
            reauthorize(&answers, &CancellationToken::new(), &|_| {})
                .await
                .expect("the change is accepted")
        }
    };

    run(&[ServiceId::Kuvert], &[]).await;
    let first = profile::read_profile(&dir)
        .unwrap()
        .config
        .service(ServiceId::Kuvert)
        .clone();
    let key = first
        .secrets
        .get("fernet")
        .cloned()
        .expect("kuvert got a fernet key");
    assert!(dir.join("secrets/kuvert.fernet").is_file());

    run(&[], &[ServiceId::Kuvert]).await;
    let removed = profile::read_profile(&dir)
        .unwrap()
        .config
        .service(ServiceId::Kuvert)
        .clone();
    assert!(!removed.enabled);
    assert_eq!(
        removed.secrets.get("fernet"),
        Some(&key),
        "the key left the profile"
    );
    assert!(!compose_services(&dir).contains(&"kuvert".to_string()));

    run(&[ServiceId::Kuvert], &[]).await;
    let back = profile::read_profile(&dir)
        .unwrap()
        .config
        .service(ServiceId::Kuvert)
        .clone();
    assert!(back.enabled && !back.retained);
    assert_eq!(
        back.secrets.get("fernet"),
        Some(&key),
        "a new key was minted"
    );
    assert_eq!(back.instance_key_pair, first.instance_key_pair);

    std::fs::remove_dir_all(&dir).ok();
}

/// A declined change is a declined authorization: the profile stays byte for byte.
#[tokio::test]
async fn a_declined_service_change_writes_nothing() {
    let dir = a_hub();
    let before = listing(&dir);
    let profile_before = std::fs::read(profile::profile_path(&dir)).unwrap();
    let server = coordination_server(declined()).await;

    reauthorize(
        &with_change(&dir, &server, &[ServiceId::Bank], &[ServiceId::Kraph]),
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .expect_err("declined");
    assert_eq!(listing(&dir), before);
    assert_eq!(
        std::fs::read(profile::profile_path(&dir)).unwrap(),
        profile_before
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// The CLI and the app build their answers from what is on disk: the hub's identifier,
/// server and addresses as authorized, and no mesh key for a hub that is not on a mesh.
#[tokio::test]
async fn service_change_answers_come_from_the_authorized_hub() {
    let dir = a_hub();
    let change = ServiceChange {
        add: vec![ServiceId::Bank],
        remove: vec![],
        ..Default::default()
    };
    let error = answers_from_disk(&dir, change.clone()).expect_err("never authorized");
    assert!(error.to_string().contains("authorize it first"), "{error}");

    let server = coordination_server(accepted(json!({}))).await;
    reauthorize(&answers(&dir, &server), &CancellationToken::new(), &|_| {})
        .await
        .expect("authorized");

    let from_disk = answers_from_disk(&dir, change.clone()).expect("answers");
    assert_eq!(from_disk.identifier, "lab-hub");
    assert_eq!(from_disk.coord_server, server.uri());
    assert_eq!(from_disk.hosts.len(), 1);
    assert_eq!(from_disk.mesh_key, MeshKeyRequest::Never);
    assert_eq!(from_disk.services, Some(change.clone()));

    // A hub authorized before its addresses were recorded would send none, withdrawing
    // every one the coordination server has: refused, with the way out.
    let mut credentials = konstruktor_core::credentials::read_credentials(&dir).unwrap();
    credentials.advertised_hosts.clear();
    konstruktor_core::credentials::write_credentials(&dir, &credentials).unwrap();
    let error = answers_from_disk(&dir, change).expect_err("no recorded addresses");
    assert!(
        error.to_string().contains("konstruktor authorize"),
        "{error}"
    );

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
