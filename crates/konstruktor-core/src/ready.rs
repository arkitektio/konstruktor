//! Whether a hub that was just started is ready to be *used*.
//!
//! `up` returns when the containers exist, which is seconds before anything answers:
//! the databases are prepared beforehand, but every server still has to come up. A
//! script that connects an app the moment `up` returns gets a refused connection and
//! concludes the hub is broken.
//!
//! This is the answer for that script: ask every endpoint a client will open, through
//! the gateway, until all of them answer or the time is up. It is deliberately lighter
//! than [`crate::health::check`], which also watches containers for crash loops over a
//! hold and is for judging a hub that was already running.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::hub::{scheme_of, HubConfig};
use crate::connect::manifest::advertised_port;
use crate::health::HEALTH_PATH;

/// How long one request gets. Short: an endpoint that is not up yet refuses at once, and
/// one that hangs is asked again on the next round anyway.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// The pause between rounds. Short: a hub is prepared before it is started, so what is
/// waited for here is servers coming up, which is a matter of seconds — and whoever waits
/// is usually a test suite.
const INTERVAL: Duration = Duration::from_millis(500);

/// What a self-contained hub's coordination server answers on once it serves. Its accounts
/// are seeded before it is started, by the job that prepares its database, so this
/// answering is the hub being ready to log in to — and it is the first thing an app asks
/// for.
pub const WELL_KNOWN: &str = ".well-known/fakts";

/// One endpoint, and what it last said.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    /// The service — `mikro`, or `s3`, or `fakts` for the coordination server's well-known.
    pub name: String,
    pub url: String,
    /// The last HTTP status, when it answered at all.
    pub status: Option<u16>,
    pub ready: bool,
    /// Why it is not ready although it answered, when that needs saying.
    #[serde(default)]
    pub detail: Option<String>,
    /// Asked with the certificate verified, as a client of a public site asks: it answers
    /// once the gateway serves a certificate a client trusts for its name.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub verified: bool,
    /// Ready, and still worth saying something about: [`Self::detail`] is a warning, not why
    /// it is not ready. What `wait` makes of a signup policy Lok does not enforce — no
    /// amount of waiting changes it, and the server is usable.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub warning: bool,
}

/// What an endpoint's answer is checked for beyond its status.
enum Expect {
    /// Nothing: any success.
    Success,
    /// A self-contained hub's well-known, following the address it was asked at.
    FollowsRequest,
    /// A coordination server's well-known, naming its origin.
    Issuer(String),
    /// Lok's account configuration, taking signups or not.
    Signup(bool),
}

/// The name of the endpoint that is a self-contained hub's well-known.
const FAKTS: &str = "fakts";

/// What a well-known that answered still has to say before the hub can be logged into:
/// that its token endpoint is at the address that was asked.
///
/// A self-contained hub tells its coordination server to advertise its endpoints at
/// whatever address a request arrived at (`discovery_follows_request`). A Lok from before
/// that setting ignores it and builds them on the issuer instead — which here is a name,
/// not an address — so it serves a well-known, with a 200, that sends every app nowhere.
/// `Err` says so; the alternative is a hub that waits ready and cannot be used.
pub fn check_well_known(asked: &str, body: &str) -> Result<(), String> {
    let base = asked.trim_end_matches(WELL_KNOWN);
    let document: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "the well-known is not JSON".to_string())?;
    let token_endpoint = document["token_endpoint"].as_str().unwrap_or_default();
    if token_endpoint.starts_with(base) {
        return Ok(());
    }
    Err(format!(
        "asked at {base}, the coordination server advertises its token endpoint at \
         `{token_endpoint}`: this Lok image does not support `discovery_follows_request`, \
         which a hub that runs its own coordination server needs — use a newer one \
         (`--image lok=…`)"
    ))
}

/// What a coordination server's well-known has to say: that it is the issuer it was made
/// as, and its endpoints are there. Lok builds them on its issuer, so a well-known that names
/// another is a Lok configured as something else — every token it mints would be refused.
pub fn check_issuer(origin: &str, body: &str) -> Result<(), String> {
    let origin = origin.trim_end_matches('/');
    let document: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "the well-known is not JSON".to_string())?;
    let issuer = document["issuer"].as_str().unwrap_or_default();
    let token_endpoint = document["token_endpoint"].as_str().unwrap_or_default();
    if issuer.trim_end_matches('/') == origin && token_endpoint.starts_with(origin) {
        return Ok(());
    }
    Err(format!(
        "the coordination server says it is `{issuer}`, with its token endpoint at \
         `{token_endpoint}`; it was made to be {origin}"
    ))
}

/// What Lok's account configuration (allauth's, `/lok/_allauth/browser/v1/config`) has to
/// say of signups.
///
/// Lok ignores a setting it does not know, so a server meant closed whose image predates the
/// switch would take anybody's signup without a word. This is what says so.
pub fn check_signup(open: bool, body: &str) -> Result<(), String> {
    let document: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| "the account configuration is not JSON".to_string())?;
    match document["data"]["account"]["is_open_for_signup"].as_bool() {
        Some(answer) if answer == open => Ok(()),
        Some(true) => Err(
            "Signups are OPEN: Lok takes anybody's, though this coordination server is meant to \
             be closed — its image does not read `account.allow_signup` yet. Until a Lok \
             release does (`konstruktor update` then takes it), anybody who reaches it can \
             make an account."
                .into(),
        ),
        Some(false) => Err(
            "Lok takes no signups, though this coordination server is meant to be open to \
             them."
                .into(),
        ),
        None => Err("Lok's account configuration does not say whether it takes signups".into()),
    }
}

/// Every endpoint the hub has to answer on before an app can use it, through the gateway
/// on this machine. `None` for a hub that publishes no port — a mesh-only one, which
/// nothing on this machine can ask.
pub fn endpoints(config: &HubConfig) -> Option<Vec<Endpoint>> {
    if config.running_coord().is_some() {
        return coord_endpoints(config).map(|e| e.into_iter().map(|(e, _)| e).collect());
    }
    config
        .gateway
        .exposed_http_port
        .or(config.gateway.exposed_https_port)?;
    let base = format!(
        "{}://localhost:{}",
        scheme_of(config),
        advertised_port(config)
    );
    let endpoint = |name: &str, path: String| Endpoint {
        name: name.to_string(),
        url: format!("{base}/{path}"),
        status: None,
        ready: false,
        detail: None,
        verified: false,
        warning: false,
    };

    let mut out = Vec::new();
    if let Some(lok) = config.running_lok() {
        out.push(endpoint(FAKTS, WELL_KNOWN.to_string()));
        out.push(endpoint(&lok.host, format!("{}/{HEALTH_PATH}", lok.host)));
    }
    for id in config.enabled_services() {
        let block = config.service(id);
        if block.image.is_some() {
            out.push(endpoint(
                &block.host,
                format!("{}/{}", block.host, block.health_path()),
            ));
        }
    }
    if config.minio.enabled {
        out.push(endpoint(
            "s3",
            crate::generate::caddy::S3_CHALLENGE.to_string(),
        ));
    }
    Some(out)
}

/// A coordination server's endpoints, each with what its answer is checked for: at its
/// public names when it has them — asked on this machine, the names resolved here — and at
/// `localhost` when it serves plain HTTP.
fn coord_endpoints(config: &HubConfig) -> Option<Vec<(Endpoint, Expect)>> {
    let coord = config.running_coord()?;
    let lok = config.running_lok()?;
    config
        .gateway
        .exposed_http_port
        .or(config.gateway.exposed_https_port)?;
    let (base, verified) = match config.public_origin() {
        Some(origin) => (origin, true),
        None => (
            format!("http://localhost:{}", advertised_port(config)),
            false,
        ),
    };
    let endpoint = |name: &str, url: String, verified: bool| Endpoint {
        name: name.to_string(),
        url,
        status: None,
        ready: false,
        detail: None,
        verified,
        warning: false,
    };
    let mut out = vec![
        (
            endpoint(FAKTS, format!("{base}/{WELL_KNOWN}"), verified),
            Expect::Issuer(coord.origin.clone()),
        ),
        (
            endpoint(
                &lok.host,
                format!("{base}/{}/{HEALTH_PATH}", lok.host),
                false,
            ),
            Expect::Success,
        ),
        (
            endpoint(
                "signup",
                format!("{base}/{}/_allauth/browser/v1/config", lok.host),
                false,
            ),
            Expect::Signup(coord.signup.is_open()),
        ),
        (
            endpoint(&coord.kontrol.host, format!("{base}/"), false),
            Expect::Success,
        ),
        (
            endpoint(
                "s3",
                format!("{base}/{}", crate::generate::caddy::S3_CHALLENGE),
                false,
            ),
            Expect::Success,
        ),
    ];
    if let Some(mesh) = coord.mesh.as_ref().filter(|_| verified) {
        out.push((
            endpoint(&mesh.host, format!("{}/healthz", mesh.url()), true),
            Expect::Success,
        ));
    }
    Some(out)
}

/// Asks every endpoint until all of them answer with a success, or `timeout` runs out.
/// Returns what each one last said either way; the caller decides what an unready one
/// means. `on_round` is told after every round, for narration.
///
/// A coordination server's public names are asked on this machine: resolved here, whatever
/// DNS says of them yet. [`check`] asks them the way the internet does.
pub async fn wait(
    config: &HubConfig,
    timeout: Duration,
    on_round: &(dyn Fn(&[Endpoint]) + Sync),
) -> Result<Vec<Endpoint>, String> {
    ask(config, timeout, true, on_round).await
}

/// Asks every endpoint once, a coordination server's public names as DNS resolves them:
/// what a client elsewhere would find.
pub async fn check(config: &HubConfig) -> Result<Vec<Endpoint>, String> {
    ask(config, Duration::ZERO, false, &|_| {}).await
}

async fn ask(
    config: &HubConfig,
    timeout: Duration,
    here: bool,
    on_round: &(dyn Fn(&[Endpoint]) + Sync),
) -> Result<Vec<Endpoint>, String> {
    let unpublished = || {
        "this hub publishes no port on this machine, so there is nothing here to ask".to_string()
    };
    let (mut endpoints, expectations): (Vec<Endpoint>, Vec<Expect>) = match coord_endpoints(config)
    {
        Some(asked) => asked.into_iter().unzip(),
        None => {
            let endpoints = endpoints(config).ok_or_else(unpublished)?;
            let expectations = endpoints
                .iter()
                .map(|e| {
                    if e.name == FAKTS {
                        Expect::FollowsRequest
                    } else {
                        Expect::Success
                    }
                })
                .collect();
            (endpoints, expectations)
        }
    };

    // A public site's names, resolved to this machine's gateway when asked from here.
    let pinned: Vec<(String, std::net::SocketAddr)> = config
        .public_site()
        .filter(|_| here)
        .map(|site| {
            let port = crate::config::hub::public_port(config);
            site.names()
                .into_iter()
                .map(|name| (name, std::net::SocketAddr::from(([127, 0, 0, 1], port))))
                .collect()
        })
        .unwrap_or_default();
    let build = |builder: reqwest::ClientBuilder| {
        pinned
            .iter()
            .fold(builder.timeout(REQUEST_TIMEOUT), |builder, (name, addr)| {
                builder.resolve(name, *addr)
            })
            .build()
            .map_err(|e| e.to_string())
    };
    // Whether the certificate is right is the gateway's business; this asks whether the
    // service answers. Same reasoning as `gateway_check`.
    let client = build(reqwest::Client::builder().danger_accept_invalid_certs(true))?;
    // Except where it is the point: a public site's, which every client verifies.
    let verifying = build(crate::trust::client_builder())?;

    let started = Instant::now();
    loop {
        // Side by side: one that hangs must not hold up the asking of the others.
        futures_util::future::join_all(
            endpoints
                .iter_mut()
                .zip(&expectations)
                .filter(|(e, _)| !e.ready)
                .map(|(endpoint, expect)| {
                    let client = if endpoint.verified {
                        &verifying
                    } else {
                        &client
                    };
                    async move {
                        match client.get(&endpoint.url).send().await {
                            Ok(response) => {
                                endpoint.status = Some(response.status().as_u16());
                                endpoint.ready = response.status().is_success();
                                if endpoint.ready && !matches!(expect, Expect::Success) {
                                    let body = response.text().await.unwrap_or_default();
                                    endpoint.detail = match expect {
                                        Expect::FollowsRequest => {
                                            check_well_known(&endpoint.url, &body).err()
                                        }
                                        Expect::Issuer(origin) => check_issuer(origin, &body).err(),
                                        Expect::Signup(open) => check_signup(*open, &body).err(),
                                        Expect::Success => None,
                                    };
                                    // Waited on, a signup policy is said, not waited for.
                                    endpoint.warning = here
                                        && matches!(expect, Expect::Signup(_))
                                        && endpoint.detail.is_some();
                                    endpoint.ready = endpoint.detail.is_none() || endpoint.warning;
                                }
                            }
                            Err(error) => {
                                endpoint.status = None;
                                // A refused certificate is worth saying: it is the one
                                // answer that will not change by waiting on the server.
                                endpoint.detail = endpoint
                                    .verified
                                    .then(|| describe_failure(&error))
                                    .flatten();
                            }
                        }
                    }
                }),
        )
        .await;
        on_round(&endpoints);
        // A well-known that answers wrongly will not answer rightly later: the image is
        // what it is. No point waiting out the clock on it. A certificate that is refused
        // may still be replaced by the one the gateway is obtaining, so that is waited on.
        let hopeless = endpoints
            .iter()
            .any(|e| !e.ready && e.detail.is_some() && e.status.is_some());
        if endpoints.iter().all(|e| e.ready) || hopeless || started.elapsed() >= timeout {
            return Ok(endpoints);
        }
        tokio::time::sleep(INTERVAL).await;
    }
}

/// Why a verified request failed, when the reason is the certificate.
fn describe_failure(error: &reqwest::Error) -> Option<String> {
    let mut source: Option<&dyn std::error::Error> = Some(error);
    while let Some(cause) = source {
        let text = cause.to_string();
        if text.contains("certificate") || text.contains("UnknownIssuer") {
            return Some(format!("its certificate is refused: {text}"));
        }
        source = cause.source();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ServiceId;
    use crate::config::hub::{build_hub_config, HubConfigOptions, LOCAL_COORD_SERVER};

    #[test]
    fn a_self_contained_hub_is_asked_for_its_well_known_first() {
        let config = build_hub_config(&HubConfigOptions {
            coord_server: LOCAL_COORD_SERVER.into(),
            services: Some(vec![ServiceId::Rekuest, ServiceId::Mikro]),
            http_port: Some(7190),
            https_port: None,
            ..Default::default()
        });
        let urls: Vec<String> = endpoints(&config)
            .unwrap()
            .into_iter()
            .map(|e| e.url)
            .collect();
        assert_eq!(
            urls,
            [
                "http://localhost:7190/.well-known/fakts",
                "http://localhost:7190/lok/ht",
                "http://localhost:7190/rekuest/ht",
                "http://localhost:7190/mikro/ht",
                "http://localhost:7190/rustfs/health",
            ]
        );
    }

    #[test]
    fn a_well_known_has_to_advertise_the_address_it_was_asked_at() {
        let asked = "http://localhost:7190/.well-known/fakts";
        let follows =
            r#"{"issuer": "lok", "token_endpoint": "http://localhost:7190/lok/o/token/"}"#;
        assert!(check_well_known(asked, follows).is_ok());

        // What a Lok without the setting builds on an issuer that is a name.
        let ignores = r#"{"issuer": "lok", "token_endpoint": "lok/lok/o/token/"}"#;
        let why = check_well_known(asked, ignores).unwrap_err();
        assert!(why.contains("discovery_follows_request"), "{why}");
        assert!(why.contains("`lok/lok/o/token/`"), "{why}");

        // Pinned somewhere else is the same failure from the client's side.
        let pinned = r#"{"token_endpoint": "http://lok/lok/o/token/"}"#;
        assert!(check_well_known(asked, pinned).is_err());
        assert!(check_well_known(asked, "<html>").is_err());
    }

    #[test]
    fn a_coordination_servers_well_known_names_the_origin_it_was_made_as() {
        let origin = "https://coord.example.org";
        let right = r#"{"issuer": "https://coord.example.org",
            "token_endpoint": "https://coord.example.org/lok/o/token/"}"#;
        assert!(check_issuer(origin, right).is_ok());
        // The issuer with Lok's prefix: every token would name an address nothing serves.
        let doubled = r#"{"issuer": "https://coord.example.org/lok",
            "token_endpoint": "https://coord.example.org/lok/lok/o/token/"}"#;
        assert!(check_issuer(origin, doubled).is_err());
        // Following the request, as a self-contained hub's does.
        let follows = r#"{"issuer": "lok", "token_endpoint": "http://localhost/lok/o/token/"}"#;
        assert!(check_issuer(origin, follows).is_err());
    }

    #[test]
    fn signups_are_what_they_were_meant_to_be_or_said_loudly() {
        let open = r#"{"data": {"account": {"is_open_for_signup": true}}}"#;
        let closed = r#"{"data": {"account": {"is_open_for_signup": false}}}"#;
        assert!(check_signup(false, closed).is_ok());
        assert!(check_signup(true, open).is_ok());
        let why = check_signup(false, open).unwrap_err();
        assert!(
            why.contains("OPEN") && why.contains("account.allow_signup"),
            "{why}"
        );
        assert!(check_signup(true, closed).is_err());
        assert!(check_signup(false, "{}").is_err());
    }

    #[test]
    fn a_hub_without_a_published_port_has_nothing_to_ask() {
        let config = build_hub_config(&HubConfigOptions {
            http_port: None,
            https_port: None,
            ..Default::default()
        });
        assert!(endpoints(&config).is_none());
    }
}
