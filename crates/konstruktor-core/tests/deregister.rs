//! A hub removing itself from a stand-in coordination server: the login it asks with,
//! what each answer means, and what is left in the folder when the server says no.
//!
//! Every hub here is one without a `reporter`, so nothing reaches for Docker — the login
//! then lives in the folder instead of a volume, and the rest is the same.

use std::path::{Path, PathBuf};

use konstruktor_core::credentials::{credentials_path, CREDENTIALS_FILENAME};
use konstruktor_core::deregister::{
    deregister, DeregisterError, ServerOutcome, DEREGISTERED_FILENAME, RESCUE_DIR,
};
use konstruktor_core::hubhealth::read_state;
use serde_json::json;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn scratch() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "konstruktor-deregister-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir
}

/// A folder holding the grant of a hub authorized against `server`.
fn authorized_hub(server: &MockServer, refresh_token: Option<&str>) -> PathBuf {
    let dir = scratch();
    let credentials = json!({
        "version": 1,
        "server": server.uri(),
        "identifier": "mylab",
        "authorizedAt": "2026-01-01T00:00:00Z",
        "envelope": {
            "token_type": "Bearer",
            "access_token": "at-0",
            "refresh_token": refresh_token,
            "client_id": "hub-client",
        },
    });
    std::fs::write(dir.join(CREDENTIALS_FILENAME), credentials.to_string()).expect("credentials");
    dir
}

async fn declares(server: &MockServer, well_known: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/.well-known/fakts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(well_known))
        .mount(server)
        .await;
}

async fn declares_a_token_endpoint(server: &MockServer) {
    declares(
        server,
        json!({ "token_endpoint": format!("{}/lok/o/token/", server.uri()) }),
    )
    .await;
}

/// The token endpoint trading `spent` for `at-1` and the rotated `rt-1`.
async fn refreshes(server: &MockServer, spent: &str) {
    Mock::given(method("POST"))
        .and(path("/lok/o/token/"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains(format!("refresh_token={spent}")))
        .and(body_string_contains("client_id=hub-client"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "at-1",
            "refresh_token": "rt-1",
            "expires_in": 3600,
            "token_type": "Bearer"
        })))
        .expect(1)
        .mount(server)
        .await;
}

fn rescued_token(dir: &Path) -> Option<String> {
    read_state(&dir.join(RESCUE_DIR)).map(|state| state.refresh_token)
}

#[tokio::test]
async fn a_hub_removes_itself_with_its_own_token() {
    let server = MockServer::start().await;
    declares_a_token_endpoint(&server).await;
    refreshes(&server, "rt-0").await;
    Mock::given(method("POST"))
        .and(path("/lok/f/hubdelete/"))
        .and(header("authorization", "Bearer at-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "deleted" })))
        .expect(1)
        .mount(&server)
        .await;

    let dir = authorized_hub(&server, Some("rt-0"));
    let outcome = deregister(&dir).await.expect("the server deletes the hub");

    assert_eq!(outcome, ServerOutcome::Removed);
    // The login is void, and a second delete must not go asking with it.
    assert!(!credentials_path(&dir).exists());
    assert!(dir.join(DEREGISTERED_FILENAME).exists());
    assert_eq!(
        deregister(&dir).await.expect("nothing left to ask"),
        ServerOutcome::NotRegistered
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn a_declared_deletion_endpoint_is_the_one_asked() {
    let server = MockServer::start().await;
    declares(
        &server,
        json!({
            "token_endpoint": format!("{}/lok/o/token/", server.uri()),
            "hub_deletion_endpoint": format!("{}/elsewhere/", server.uri()),
        }),
    )
    .await;
    refreshes(&server, "rt-0").await;
    Mock::given(method("POST"))
        .and(path("/elsewhere/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "deleted" })))
        .expect(1)
        .mount(&server)
        .await;

    let dir = authorized_hub(&server, Some("rt-0"));
    assert_eq!(
        deregister(&dir).await.expect("deleted"),
        ServerOutcome::Removed
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// Somebody removed the hub on the server first. That is the state the delete wanted.
#[tokio::test]
async fn a_hub_the_server_no_longer_has_counts_as_gone() {
    let server = MockServer::start().await;
    declares_a_token_endpoint(&server).await;
    refreshes(&server, "rt-0").await;
    Mock::given(method("POST"))
        .and(path("/lok/f/hubdelete/"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "status": "error",
            "error": "hub_not_found",
            "error_description": "No hub found for this token"
        })))
        .mount(&server)
        .await;

    let dir = authorized_hub(&server, Some("rt-0"));
    assert_eq!(
        deregister(&dir).await.expect("already gone"),
        ServerOutcome::AlreadyGone
    );
    assert!(!credentials_path(&dir).exists());

    std::fs::remove_dir_all(&dir).ok();
}

/// A server from before the endpoint existed answers 404 too, and must not be taken for
/// "gone". The refresh has rotated the token by then, so the new one has to survive for
/// the hub to keep its login — and for the next attempt to ask with.
#[tokio::test]
async fn a_server_without_the_endpoint_is_not_mistaken_for_a_removed_hub() {
    let server = MockServer::start().await;
    declares_a_token_endpoint(&server).await;
    refreshes(&server, "rt-0").await;
    // No hubdelete route: wiremock answers 404 with no body, as an older server would
    // with an HTML page.

    let dir = authorized_hub(&server, Some("rt-0"));
    let refused = deregister(&dir).await;

    assert!(
        matches!(refused, Err(DeregisterError::Unsupported(_))),
        "expected a refusal, got {refused:?}"
    );
    assert!(
        credentials_path(&dir).exists(),
        "the hub is still registered"
    );
    assert_eq!(rescued_token(&dir).as_deref(), Some("rt-1"));

    // The retry asks with the rotated token, not the spent one in the credentials.
    refreshes(&server, "rt-1").await;
    Mock::given(method("POST"))
        .and(path("/lok/f/hubdelete/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "deleted" })))
        .mount(&server)
        .await;
    assert_eq!(
        deregister(&dir).await.expect("deleted"),
        ServerOutcome::Removed
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn a_refused_login_leaves_the_hub_as_it_was() {
    let server = MockServer::start().await;
    declares_a_token_endpoint(&server).await;
    Mock::given(method("POST"))
        .and(path("/lok/o/token/"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "the refresh token was revoked"
        })))
        .mount(&server)
        .await;

    let dir = authorized_hub(&server, Some("rt-0"));
    let refused = deregister(&dir).await;

    assert!(
        matches!(refused, Err(DeregisterError::InvalidGrant(_))),
        "expected a refusal, got {refused:?}"
    );
    assert!(credentials_path(&dir).exists());
    assert!(!dir.join(RESCUE_DIR).exists(), "nothing was rotated");

    std::fs::remove_dir_all(&dir).ok();
}

/// A grant without a refresh token has only the access token it came with.
#[tokio::test]
async fn a_hub_without_a_refresh_token_asks_with_what_it_has() {
    let server = MockServer::start().await;
    declares_a_token_endpoint(&server).await;
    Mock::given(method("POST"))
        .and(path("/lok/f/hubdelete/"))
        .and(header("authorization", "Bearer at-0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "deleted" })))
        .expect(1)
        .mount(&server)
        .await;

    let dir = authorized_hub(&server, None);
    assert_eq!(
        deregister(&dir).await.expect("deleted"),
        ServerOutcome::Removed
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn an_expired_token_is_a_refusal_not_a_removal() {
    let server = MockServer::start().await;
    declares_a_token_endpoint(&server).await;
    Mock::given(method("POST"))
        .and(path("/lok/f/hubdelete/"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "token expired"
        })))
        .mount(&server)
        .await;

    let dir = authorized_hub(&server, None);
    let refused = deregister(&dir).await;

    assert!(
        matches!(refused, Err(DeregisterError::Unauthorized(_))),
        "expected a refusal, got {refused:?}"
    );
    assert!(credentials_path(&dir).exists());

    std::fs::remove_dir_all(&dir).ok();
}

/// A hub that was never authorized has no server to ask, and none is.
#[tokio::test]
async fn an_unauthorized_hub_has_nothing_to_remove() {
    let dir = scratch();
    assert_eq!(
        deregister(&dir).await.expect("nothing to do"),
        ServerOutcome::NotRegistered
    );
    std::fs::remove_dir_all(&dir).ok();
}
