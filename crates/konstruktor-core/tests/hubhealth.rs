//! The reporter against a stand-in coordination server: the token refresh, the rotation it
//! must never lose, and the report itself.

use std::path::PathBuf;
use std::time::Duration;

use konstruktor_core::hubhealth::{
    read_state, refresh, send_report, HealthReport, MeshReport, ReportError, ReporterState,
};
use serde_json::json;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "konstruktor-hubhealth-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir
}

fn rand_suffix() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn state() -> ReporterState {
    ReporterState {
        client_id: "hub-client".into(),
        refresh_token: "rt-0".into(),
        seeded_from: "hub-client".into(),
    }
}

/// The refresh token rotates on every use. The new one has to be on disk before the
/// access token is used, or a reporter killed at the wrong moment has no way back in.
#[tokio::test]
async fn a_refresh_persists_the_rotated_token() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/lok/o/token/"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=rt-0"))
        .and(body_string_contains("client_id=hub-client"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "at-1",
            "refresh_token": "rt-1",
            "expires_in": 3600,
            "token_type": "Bearer"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let dir = scratch();
    let mut current = state();
    let access = refresh(
        &reqwest::Client::new(),
        &format!("{}/lok/o/token/", server.uri()),
        &mut current,
        &dir,
    )
    .await
    .expect("refreshed");

    assert_eq!(access.token, "at-1");
    assert_eq!(current.refresh_token, "rt-1");
    assert_eq!(read_state(&dir).expect("state on disk").refresh_token, "rt-1");
    std::fs::remove_dir_all(&dir).ok();
}

/// A replaced or revoked login is not a transport hiccup: it says to authorize again.
#[tokio::test]
async fn a_refused_refresh_token_is_named_as_such() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/lok/o/token/"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "Invalid refresh token"
        })))
        .mount(&server)
        .await;

    let dir = scratch();
    let mut current = state();
    let error = refresh(
        &reqwest::Client::new(),
        &format!("{}/lok/o/token/", server.uri()),
        &mut current,
        &dir,
    )
    .await
    .err()
    .expect("refused");

    assert!(matches!(error, ReportError::InvalidGrant(_)), "got {error:?}");
    // Nothing was rotated, so nothing was written.
    assert_eq!(current.refresh_token, "rt-0");
    assert!(read_state(&dir).is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn a_report_is_sent_as_the_hub_and_says_when_to_come_back() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/lok/f/hubhealth/"))
        .and(header("authorization", "Bearer at-1"))
        .and(body_string_contains(r#""healthy":true"#))
        .and(body_string_contains(r#""hostname":"myhub.iac.mesh.arkitekt.live""#))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "reported",
            "message": "Health report processed successfully",
            "next_report_in": 90
        })))
        .expect(1)
        .mount(&server)
        .await;

    let next = send_report(
        &reqwest::Client::new(),
        &format!("{}/lok/f/hubhealth/", server.uri()),
        "at-1",
        &HealthReport {
            healthy: true,
            version: "0.5.1".into(),
            mesh: Some(MeshReport {
                connected: true,
                hostname: Some("myhub.iac.mesh.arkitekt.live".into()),
                ipv4: Some("100.64.0.5".into()),
            }),
        },
    )
    .await
    .expect("reported");

    assert_eq!(next, Duration::from_secs(90));
}

/// An access token the server no longer takes is told apart from every other failure, so
/// the loop knows a fresh token — not a longer wait — is the fix.
#[tokio::test]
async fn an_expired_access_token_is_unauthorized_not_a_server_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/lok/f/hubhealth/"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "status": "error",
            "error": "invalid_grant",
            "error_description": "Token expired"
        })))
        .mount(&server)
        .await;

    let error = send_report(
        &reqwest::Client::new(),
        &format!("{}/lok/f/hubhealth/", server.uri()),
        "at-stale",
        &HealthReport {
            healthy: true,
            version: "0.5.1".into(),
            mesh: None,
        },
    )
    .await
    .err()
    .expect("refused");

    assert!(matches!(error, ReportError::Unauthorized(ref d) if d == "Token expired"), "got {error:?}");
}
