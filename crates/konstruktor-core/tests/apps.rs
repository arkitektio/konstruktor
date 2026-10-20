//! An app's release, read from its registry as `app attach` reads it.

use konstruktor_core::apps::{self, CapabilityKind, Host};

/// The plugin engine's own release on ghcr.io: its flavour, and the container engine it asks
/// to be granted. Needs the network, so it runs only when asked
/// (`cargo test --test apps -- --ignored`).
#[tokio::test]
#[ignore]
async fn the_plugin_engine_release_asks_for_the_container_engine() {
    let (reference, release) = apps::release(apps::PLUGIN_ENGINE_RELEASE)
        .await
        .expect("the plugin engine has a release");
    assert!(
        reference.starts_with("ghcr.io/arkitektio/plugin_engine:"),
        "{reference}"
    );
    assert_eq!(release.manifest.identifier, "plugin_engine");

    let host = Host {
        architecture: "amd64".into(),
        nvidia: false,
        amd: false,
    };
    let flavour = apps::choose(&release, None, &host).expect("a flavour runs on amd64");
    assert!(flavour.image.contains("@sha256:"), "{}", flavour.image);
    assert!(flavour
        .capabilities
        .iter()
        .any(|c| matches!(c.kind, CapabilityKind::ContainerEngine { .. })));
}

// --- attaching, against stand-ins -------------------------------------------------------

use std::path::PathBuf;

use konstruktor_core::apps::AttachAnswers;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const DESCRIPTOR: &str = "sha256:5555555555555555555555555555555555555555555555555555555555555555";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("konstruktor-apps-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir
}

/// One stand-in for both: the registry the release is read from, and the coordination
/// server that accepts the app into `organization`.
async fn stand_in(organization: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/org/engine/manifests/2.1.0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {"digest": DESCRIPTOR, "mediaType": "application/vnd.arkitekt.release.v1+json"},
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/org/engine/blobs/{DESCRIPTOR}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "spec_version": 1,
            "manifest": {"identifier": "plugin_engine", "version": "2.1.0", "scopes": ["read", "write"]},
            "flavours": [{
                "name": "vanilla",
                "image": "ghcr.io/org/engine@sha256:aaaa",
                "platforms": ["linux/amd64", "linux/arm64"],
                "capabilities": [{"kind": "container_engine", "name": "engine"}],
                "inspection": {"requirements": [{"key": "kabinet", "service": "live.arkitekt.kabinet"}]},
            }],
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/.well-known/fakts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "issuer": "https://coord.example.org",
            "device_authorization_endpoint": format!("{}/o/device/", server.uri()),
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/o/device/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_code": "dc",
            "user_code": "B4X2",
            "token_endpoint": format!("{}/o/token/", server.uri()),
            "verification_uri_complete": format!("{}/configure/B4X2", server.uri()),
            "expires_in": 300,
            "interval": 1,
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/o/token/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token_type": "Bearer",
            "access_token": "eyJ",
            "refresh_token": "rt-app",
            "client_id": "engine-client",
            "self": {"sub": "2", "organization": organization},
        })))
        .mount(&server)
        .await;
    server
}

/// A hub on this machine, connected to `server` in organization 3.
fn a_connected_hub(server: &MockServer, name: &str) -> PathBuf {
    let dir = scratch(&format!("hub-{name}"));
    let config = konstruktor_core::config::hub::build_hub_config(&Default::default());
    konstruktor_core::profile::write_profile(&dir, &konstruktor_core::profile::hub_profile(config))
        .expect("a profile");
    std::fs::write(
        dir.join(konstruktor_core::credentials::CREDENTIALS_FILENAME),
        json!({
            "version": 2,
            "server": server.uri(),
            "identifier": "lab",
            "authorizedAt": "now",
            "envelope": {"token_type": "Bearer", "access_token": "a", "client_id": "hub", "self": {"organization": "3"}},
        })
        .to_string(),
    )
    .expect("credentials");
    dir
}

fn answers(server: &MockServer, hub: &std::path::Path, dir: &std::path::Path) -> AttachAnswers {
    AttachAnswers {
        dir: dir.to_string_lossy().to_string(),
        name: "engine".into(),
        release: format!("{}/org/engine:2.1.0", server.address()),
        hub: Some(hub.to_string_lossy().to_string()),
        instance_id: Some("lab-engine".into()),
        allow: vec!["engine".into()],
        start: false,
        ..Default::default()
    }
}

async fn docker_is_ready() -> bool {
    std::env::set_var(
        konstruktor_core::registry::DATA_DIR_ENV,
        std::env::temp_dir().join(format!("konstruktor-apps-data-{}", std::process::id())),
    );
    konstruktor_core::docker::probe().await.is_ready()
}

#[tokio::test]
async fn an_app_accepted_into_the_hubs_organization_is_written_with_what_it_was_granted() {
    if !docker_is_ready().await {
        eprintln!("skipped: attaching checks Docker first, and there is none ready here");
        return;
    }
    let server = stand_in("3").await;
    let hub = a_connected_hub(&server, "engine");
    let dir = scratch("engine");

    let attached = apps::attach_app(
        &answers(&server, &hub, &dir),
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .expect("attached");
    assert_eq!(attached.organization.as_deref(), Some("3"));
    assert_eq!(attached.flavour, "vanilla");

    let compose: serde_norway::Value =
        serde_norway::from_str(&std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap())
            .unwrap();
    let app = &compose["services"]["app"];
    assert_eq!(
        app["image"].as_str(),
        Some("ghcr.io/org/engine@sha256:aaaa")
    );
    assert_eq!(
        app["environment"]["ARKITEKT_INSTANCE_ID"].as_str(),
        Some("lab-engine")
    );
    assert_eq!(
        app["environment"]["DOCKER_HOST"].as_str(),
        Some("unix:///var/run/docker.sock")
    );
    // The stand-in is plain HTTP, which fakts sends a credential to only when told it may.
    assert_eq!(
        app["environment"]["FAKTS_ALLOW_INSECURE_TRANSPORT"].as_str(),
        Some("1")
    );
    let secrets = std::fs::read_to_string(dir.join(apps::SECRETS_FILE)).unwrap();
    assert_eq!(secrets.trim(), "FAKTS_TOKEN=engine-client:rt-app");
    assert_eq!(
        apps::app_record(&dir, "app", "release").as_deref(),
        Some(answers(&server, &hub, &dir).release.as_str())
    );

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&hub).ok();
}

#[tokio::test]
async fn an_app_accepted_into_another_organization_writes_nothing() {
    if !docker_is_ready().await {
        eprintln!("skipped: attaching checks Docker first, and there is none ready here");
        return;
    }
    let server = stand_in("4").await;
    let hub = a_connected_hub(&server, "elsewhere");
    let dir = scratch("elsewhere");

    let error = apps::attach_app(
        &answers(&server, &hub, &dir),
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .err()
    .expect("refused");
    assert!(error.to_string().contains("organization 4"), "{error}");
    assert!(!dir.join("docker-compose.yaml").exists());

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&hub).ok();
}

#[tokio::test]
async fn the_container_engine_is_not_granted_without_being_named() {
    if !docker_is_ready().await {
        eprintln!("skipped: attaching checks Docker first, and there is none ready here");
        return;
    }
    let server = stand_in("3").await;
    let hub = a_connected_hub(&server, "unallowed");
    let dir = scratch("unallowed");
    let mut asked = answers(&server, &hub, &dir);
    asked.allow.clear();

    let error = apps::attach_app(&asked, &CancellationToken::new(), &|_| {})
        .await
        .err()
        .expect("refused");
    assert!(error.to_string().contains("--allow engine"), "{error}");
    // Nobody was sent to a browser.
    assert!(server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .all(|r| r.url.path() != "/o/device/"));

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&hub).ok();
}

// --- updating, against the same stand-ins ------------------------------------------------

const DESCRIPTOR_2_2: &str =
    "sha256:6666666666666666666666666666666666666666666666666666666666666666";
const DESCRIPTOR_2_3: &str =
    "sha256:7777777777777777777777777777777777777777777777777777777777777777";

/// The releases after 2.1.0: 2.2.0 asks for what 2.1.0 did, 2.3.0 also for a folder it may
/// write to. And the second acceptance: into `organization`, with a new refresh token.
async fn later_releases(server: &MockServer, organization: &str) {
    Mock::given(method("GET"))
        .and(path("/v2/org/engine/tags/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "org/engine",
            "tags": ["2.1.0", "2.2.0", "latest", "next"],
        })))
        .mount(server)
        .await;
    for (version, digest, image, extra) in [
        (
            "2.2.0",
            DESCRIPTOR_2_2,
            "ghcr.io/org/engine@sha256:bbbb",
            json!(null),
        ),
        (
            "2.3.0",
            DESCRIPTOR_2_3,
            "ghcr.io/org/engine@sha256:cccc",
            json!({"kind": "folder", "name": "data", "target": "/data", "readOnly": false}),
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/v2/org/engine/manifests/{version}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "schemaVersion": 2,
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "config": {"digest": digest, "mediaType": "application/vnd.arkitekt.release.v1+json"},
            })))
            .mount(server)
            .await;
        let mut capabilities = vec![json!({"kind": "container_engine", "name": "engine"})];
        if !extra.is_null() {
            capabilities.push(extra);
        }
        Mock::given(method("GET"))
            .and(path(format!("/v2/org/engine/blobs/{digest}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "spec_version": 1,
                "manifest": {"identifier": "plugin_engine", "version": version, "scopes": ["read", "write"]},
                "flavours": [{
                    "name": "vanilla",
                    "image": image,
                    "platforms": ["linux/amd64", "linux/arm64"],
                    "capabilities": capabilities,
                    "inspection": {"requirements": [{"key": "kabinet", "service": "live.arkitekt.kabinet"}]},
                }],
            })))
            .mount(server)
            .await;
    }
    // Mounted after the first acceptance, and ahead of it from then on.
    Mock::given(method("POST"))
        .and(path("/o/token/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token_type": "Bearer",
            "access_token": "eyJ",
            "refresh_token": "rt-updated",
            "client_id": "engine-client-2",
            "self": {"sub": "2", "organization": organization},
        })))
        .with_priority(1)
        .mount(server)
        .await;
}

/// An engine attached at 2.1.0 beside a hub in organization 3, not started.
async fn attached(server: &MockServer, name: &str) -> (PathBuf, PathBuf) {
    let hub = a_connected_hub(server, name);
    let dir = scratch(name);
    apps::attach_app(
        &answers(server, &hub, &dir),
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .expect("attached");
    (hub, dir)
}

fn update_answers(dir: &std::path::Path) -> apps::UpdateAnswers {
    apps::UpdateAnswers {
        dir: dir.to_string_lossy().to_string(),
        ..Default::default()
    }
}

fn compose_of(dir: &std::path::Path) -> serde_norway::Value {
    serde_norway::from_str(&std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap())
        .unwrap()
}

#[tokio::test]
async fn an_update_is_accepted_again_and_keeps_the_installation() {
    if !docker_is_ready().await {
        eprintln!("skipped: updating checks Docker first, and there is none ready here");
        return;
    }
    let server = stand_in("3").await;
    let (hub, dir) = attached(&server, "update").await;
    later_releases(&server, "3").await;

    assert_eq!(apps::available_update(&dir).await.as_deref(), Some("2.2.0"));
    let plan = apps::plan_update(&dir, Some("2.2.0"))
        .await
        .expect("a plan");
    assert!(plan.is_newer());
    // The container engine it was granted is asked for unchanged: kept, not asked again.
    assert_eq!(plan.kept, ["engine"]);
    assert!(plan.asked.is_empty() && plan.dropped.is_empty());

    let updated = apps::update_app(
        &plan,
        &update_answers(&dir),
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .expect("updated");
    assert_eq!(
        (updated.from.as_str(), updated.to.as_str()),
        ("2.1.0", "2.2.0")
    );
    assert_eq!(updated.organization.as_deref(), Some("3"));

    let app = &compose_of(&dir)["services"]["app"];
    assert_eq!(
        app["image"].as_str(),
        Some("ghcr.io/org/engine@sha256:bbbb")
    );
    // The same installation: what it started stays its own.
    assert_eq!(
        app["environment"]["ARKITEKT_INSTANCE_ID"].as_str(),
        Some("lab-engine")
    );
    assert_eq!(
        app["environment"]["DOCKER_HOST"].as_str(),
        Some("unix:///var/run/docker.sock")
    );
    // A new login for the new release: the session it keeps is its old version's.
    let secrets = std::fs::read_to_string(dir.join(apps::SECRETS_FILE)).unwrap();
    assert_eq!(secrets.trim(), "FAKTS_TOKEN=engine-client-2:rt-updated");
    let record = apps::read_app_record(&dir).expect("the record");
    assert_eq!(record.app.version, "2.2.0");
    assert_eq!(record.app.organization.as_deref(), Some("3"));

    // Up to date now.
    assert_eq!(apps::available_update(&dir).await, None);
    let again = apps::plan_update(&dir, Some("2.2.0"))
        .await
        .expect("a plan");
    assert!(!again.is_newer());

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&hub).ok();
}

#[tokio::test]
async fn what_an_update_asks_for_anew_is_granted_only_by_name() {
    if !docker_is_ready().await {
        eprintln!("skipped: updating checks Docker first, and there is none ready here");
        return;
    }
    let server = stand_in("3").await;
    let (hub, dir) = attached(&server, "anew").await;
    later_releases(&server, "3").await;

    let plan = apps::plan_update(&dir, Some("2.3.0"))
        .await
        .expect("a plan");
    assert_eq!(plan.kept, ["engine"]);
    assert_eq!(
        plan.asked
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["data"]
    );

    let error = apps::update_app(
        &plan,
        &update_answers(&dir),
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .err()
    .expect("refused");
    assert!(error.to_string().contains("--folder data="), "{error}");
    // Refused before anybody was asked to accept it, and nothing changed.
    let device_codes = server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.url.path() == "/o/device/")
        .count();
    assert_eq!(device_codes, 1, "only the attach asked");
    assert_eq!(apps::read_app_record(&dir).unwrap().app.version, "2.1.0");

    let folder = scratch("anew-data");
    let mut answers = update_answers(&dir);
    answers.allow = vec!["data".into()];
    answers
        .folders
        .insert("data".into(), folder.to_string_lossy().to_string());
    apps::update_app(&plan, &answers, &CancellationToken::new(), &|_| {})
        .await
        .expect("updated");
    let volumes = compose_of(&dir)["services"]["app"]["volumes"].clone();
    let mounted = format!("{}:/data", folder.display());
    assert!(
        volumes
            .as_sequence()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some(mounted.as_str())),
        "{volumes:?}"
    );

    for path in [&dir, &hub, &folder] {
        std::fs::remove_dir_all(path).ok();
    }
}

#[tokio::test]
async fn an_update_accepted_into_another_organization_changes_nothing() {
    if !docker_is_ready().await {
        eprintln!("skipped: updating checks Docker first, and there is none ready here");
        return;
    }
    let server = stand_in("3").await;
    let (hub, dir) = attached(&server, "moved").await;
    later_releases(&server, "4").await;
    let before = std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap();

    let plan = apps::plan_update(&dir, None).await.expect("a plan");
    assert_eq!(plan.release.manifest.version, "2.2.0", "the newest release");
    let error = apps::update_app(
        &plan,
        &update_answers(&dir),
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .err()
    .expect("refused");
    assert!(error.to_string().contains("organization 4"), "{error}");
    assert_eq!(
        std::fs::read_to_string(dir.join("docker-compose.yaml")).unwrap(),
        before
    );
    let secrets = std::fs::read_to_string(dir.join(apps::SECRETS_FILE)).unwrap();
    assert_eq!(secrets.trim(), "FAKTS_TOKEN=engine-client:rt-app");

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&hub).ok();
}

#[tokio::test]
async fn an_app_on_the_mesh_stays_the_node_it_is_through_an_update() {
    if !docker_is_ready().await {
        eprintln!("skipped: updating checks Docker first, and there is none ready here");
        return;
    }
    // Accepted onto the mesh at attach: the first acceptance brings a key.
    let server = stand_in("3").await;
    Mock::given(method("POST"))
        .and(path("/o/token/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token_type": "Bearer",
            "access_token": "eyJ",
            "refresh_token": "rt-app",
            "client_id": "engine-client",
            "self": {"sub": "2", "organization": "3"},
            "mesh": {"ionscale_auth_key": "tskey-first", "ionscale_coord_url": "https://mesh.example.org"},
        })))
        .up_to_n_times(1)
        .with_priority(2)
        .mount(&server)
        .await;
    let hub = a_connected_hub(&server, "mesh");
    let dir = scratch("mesh");
    let mut asked = answers(&server, &hub, &dir);
    asked.mesh = true;
    apps::attach_app(&asked, &CancellationToken::new(), &|_| {})
        .await
        .expect("attached on the mesh");
    let node = std::fs::read_to_string(dir.join("configs/mesh.yaml")).expect("its mesh block");
    let before = compose_of(&dir);
    assert!(
        before["services"]["app"]["network_mode"]
            .as_str()
            .is_some_and(|mode| mode.starts_with("service:")),
        "{before:?}"
    );

    // The update's acceptance brings no key: the app keeps the node it is.
    later_releases(&server, "3").await;
    let plan = apps::plan_update(&dir, Some("2.2.0"))
        .await
        .expect("a plan");
    apps::update_app(
        &plan,
        &update_answers(&dir),
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .expect("updated without a new mesh key");
    let after = compose_of(&dir);
    assert_eq!(
        after["services"]["app"]["network_mode"], before["services"]["app"]["network_mode"],
        "still inside its sidecar's network"
    );
    let sidecar = before["services"]["app"]["network_mode"]
        .as_str()
        .unwrap()
        .trim_start_matches("service:")
        .to_string();
    assert_eq!(
        after["services"][&sidecar], before["services"][&sidecar],
        "the same sidecar"
    );
    let node_after = std::fs::read_to_string(dir.join("configs/mesh.yaml")).unwrap();
    assert_eq!(node, node_after, "the same mesh block");
    // And the update asked for no key.
    let device_starts: Vec<serde_json::Value> = server
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.url.path() == "/o/device/")
        .map(|r| serde_json::from_slice(&r.body).unwrap_or_default())
        .collect();
    assert_eq!(device_starts.len(), 2, "attached, then updated");
    assert_eq!(
        device_starts[0]["request_auth_key"], true,
        "attaching asked for one"
    );
    assert!(
        device_starts[1].get("request_auth_key").is_none(),
        "{}",
        device_starts[1]
    );

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&hub).ok();
}
