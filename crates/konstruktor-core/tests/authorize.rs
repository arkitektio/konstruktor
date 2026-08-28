use konstruktor_core::connect::authorize::{poll_once, start, HubAuthorizationError, PollStatus};
use konstruktor_core::connect::manifest::{HubManifest, HubStartRequest};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The device-code flow is the one piece with no golden file, so the contracts that
/// matter get asserted directly against a stub server.

fn request() -> HubStartRequest {
    HubStartRequest {
        hub: HubManifest {
            identifier: "lab-hub".into(),
            description: None,
            logo: None,
            instances: vec![],
            clients: vec![],
            request_auth_key: false,
        },
        expiration_time_seconds: 600,
    }
}

async fn server_with_well_known(extra: serde_json::Value) -> MockServer {
    let server = MockServer::start().await;
    let mut well_known = json!({
        "issuer": "https://coord.example.org",
        "hub_authorization_endpoint": format!("{}/o/hub-authorization/", server.uri()),
    });
    if let (Some(a), Some(b)) = (well_known.as_object_mut(), extra.as_object()) {
        for (k, v) in b {
            a.insert(k.clone(), v.clone());
        }
    }
    Mock::given(method("GET"))
        .and(path("/.well-known/fakts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(well_known))
        .mount(&server)
        .await;
    server
}

fn granted_body(server: &MockServer) -> serde_json::Value {
    json!({
        "status": "granted",
        "device_code": "kJ8f-full-entropy",
        "user_code": "A7K3",
        "client_id": "9c1d",
        "token_endpoint": format!("{}/o/token/", server.uri()),
        "verification_uri": format!("{}/hubconfigure/", server.uri()),
        "verification_uri_complete": format!("{}/hubconfigure/A7K3", server.uri()),
        "expires_in": 300,
        "interval": 5
    })
}

#[tokio::test]
async fn carries_the_issuer_from_the_well_known_not_the_endpoint() {
    let server = server_with_well_known(json!({})).await;
    Mock::given(method("POST"))
        .and(path("/o/hub-authorization/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(granted_body(&server)))
        .mount(&server)
        .await;

    let grant = start(&server.uri(), &request()).await.expect("staged");

    // Authentikate matches `iss` by strict equality; guessing it from the hostname would
    // make the hub reject every token the coordination server issues.
    assert_eq!(grant.issuer.as_deref(), Some("https://coord.example.org"));
    assert_eq!(grant.device_code, "kJ8f-full-entropy");
}

/// The endpoint answers 200 with `status: "error"` for a manifest it will not take, so
/// a 2xx alone is not acceptance.
#[tokio::test]
async fn a_two_hundred_is_not_acceptance_without_granted_status() {
    let server = server_with_well_known(json!({})).await;
    Mock::given(method("POST"))
        .and(path("/o/hub-authorization/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "error",
            "error_description": "that identifier is taken"
        })))
        .mount(&server)
        .await;

    let error = start(&server.uri(), &request()).await.unwrap_err();
    assert!(
        matches!(&error, HubAuthorizationError::Refused(d) if d.contains("identifier is taken")),
        "got {error:?}"
    );
}

#[tokio::test]
async fn refuses_a_server_that_does_not_offer_hub_authorization() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/fakts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"issuer": "x"})))
        .mount(&server)
        .await;

    let error = start(&server.uri(), &request()).await.unwrap_err();
    assert!(matches!(error, HubAuthorizationError::Server(_)), "got {error:?}");
}

async fn grant_for(server: &MockServer) -> konstruktor_core::connect::authorize::HubGrant {
    Mock::given(method("POST"))
        .and(path("/o/hub-authorization/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(granted_body(server)))
        .mount(server)
        .await;
    start(&server.uri(), &request()).await.expect("staged")
}

#[tokio::test]
async fn reads_pending_slow_down_and_denial_off_the_token_endpoint() {
    for (body, check) in [
        (json!({"error": "authorization_pending"}), "pending"),
        (json!({"error": "slow_down"}), "slow"),
        (json!({"error": "access_denied"}), "denied"),
        (json!({"error": "expired_token"}), "expired"),
    ] {
        let server = server_with_well_known(json!({})).await;
        let grant = grant_for(&server).await;
        Mock::given(method("POST"))
            .and(path("/o/token/"))
            .respond_with(ResponseTemplate::new(400).set_body_json(body))
            .mount(&server)
            .await;

        match (check, poll_once(&grant).await) {
            ("pending", Ok(PollStatus::Pending)) => {}
            // Recomputed from the original grant, so repeated slow-downs do not compound.
            ("slow", Ok(PollStatus::SlowDown { interval })) => assert_eq!(interval, 10),
            ("denied", Err(HubAuthorizationError::Declined)) => {}
            ("expired", Err(HubAuthorizationError::Expired)) => {}
            (what, other) => panic!("{what}: unexpected {other:?}", other = other.map(|_| "ok")),
        }
    }
}

/// A grant with no JWKS URL would produce services that trust nothing. `poll_once`
/// casts whatever a 2xx returns, so the check has to happen before anything is written.
#[tokio::test]
async fn refuses_a_grant_that_carries_no_jwks_url() {
    let server = server_with_well_known(json!({})).await;
    let grant = grant_for(&server).await;
    Mock::given(method("POST"))
        .and(path("/o/token/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token_type": "Bearer",
            "access_token": "eyJ",
            "client_id": "9c1d",
            "auth": {}
        })))
        .mount(&server)
        .await;

    let error = konstruktor_core::connect::authorize::wait_for_hub(
        &grant,
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .unwrap_err();

    assert!(matches!(error, HubAuthorizationError::NoJwksUrl), "got {error:?}");
}

#[tokio::test]
async fn returns_the_envelope_and_any_mesh_key_with_it() {
    let server = server_with_well_known(json!({})).await;
    let grant = grant_for(&server).await;
    Mock::given(method("POST"))
        .and(path("/o/token/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "token_type": "Bearer",
            "access_token": "eyJ",
            "client_id": "9c1d",
            "auth": {
                "jwks_url": "https://coord.example.org/.well-known/jwks.json",
                "ionscale_auth_key": "tskey-auth-minted",
                "ionscale_coord_url": "https://mesh.example.org"
            }
        })))
        .mount(&server)
        .await;

    let envelope = konstruktor_core::connect::authorize::wait_for_hub(
        &grant,
        &CancellationToken::new(),
        &|_| {},
    )
    .await
    .expect("granted");

    assert_eq!(
        envelope.auth.jwks_url.as_deref(),
        Some("https://coord.example.org/.well-known/jwks.json")
    );
    assert_eq!(envelope.auth.ionscale_auth_key.as_deref(), Some("tskey-auth-minted"));
}

/// Ctrl-C during a poll interval used to land up to `interval` seconds late; the wait is
/// abort-aware now.
#[tokio::test]
async fn a_cancelled_wait_returns_immediately() {
    let server = server_with_well_known(json!({})).await;
    let grant = grant_for(&server).await;
    let cancel = CancellationToken::new();
    cancel.cancel();

    let error =
        konstruktor_core::connect::authorize::wait_for_hub(&grant, &cancel, &|_| {})
            .await
            .unwrap_err();
    assert!(matches!(error, HubAuthorizationError::Cancelled), "got {error:?}");
}
