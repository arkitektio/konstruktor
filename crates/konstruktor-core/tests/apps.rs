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
