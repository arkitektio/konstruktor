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

/// Every endpoint the hub has to answer on before an app can use it, through the gateway
/// on this machine. `None` for a hub that publishes no port — a mesh-only one, which
/// nothing on this machine can ask.
pub fn endpoints(config: &HubConfig) -> Option<Vec<Endpoint>> {
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
                format!("{}/{HEALTH_PATH}", block.host),
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

/// Asks every endpoint until all of them answer with a success, or `timeout` runs out.
/// Returns what each one last said either way; the caller decides what an unready one
/// means. `on_round` is told after every round, for narration.
pub async fn wait(
    config: &HubConfig,
    timeout: Duration,
    on_round: &(dyn Fn(&[Endpoint]) + Sync),
) -> Result<Vec<Endpoint>, String> {
    let mut endpoints = endpoints(config).ok_or_else(|| {
        "this hub publishes no port on this machine, so there is nothing here to ask".to_string()
    })?;

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        // Whether the certificate is right is the gateway's business; this asks whether
        // the service answers. Same reasoning as `gateway_check`.
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| e.to_string())?;

    let started = Instant::now();
    loop {
        // Side by side: one that hangs must not hold up the asking of the others.
        futures_util::future::join_all(endpoints.iter_mut().filter(|e| !e.ready).map(
            |endpoint| async {
                match client.get(&endpoint.url).send().await {
                    Ok(response) => {
                        endpoint.status = Some(response.status().as_u16());
                        endpoint.ready = response.status().is_success();
                        if endpoint.ready && endpoint.name == FAKTS {
                            let body = response.text().await.unwrap_or_default();
                            endpoint.detail = check_well_known(&endpoint.url, &body).err();
                            endpoint.ready = endpoint.detail.is_none();
                        }
                    }
                    Err(_) => endpoint.status = None,
                }
            },
        ))
        .await;
        on_round(&endpoints);
        // A well-known that answers wrongly will not answer rightly later: the image is
        // what it is. No point waiting out the clock on it.
        let hopeless = endpoints.iter().any(|e| e.detail.is_some());
        if endpoints.iter().all(|e| e.ready) || hopeless || started.elapsed() >= timeout {
            return Ok(endpoints);
        }
        tokio::time::sleep(INTERVAL).await;
    }
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
    fn a_hub_without_a_published_port_has_nothing_to_ask() {
        let config = build_hub_config(&HubConfigOptions {
            http_port: None,
            https_port: None,
            ..Default::default()
        });
        assert!(endpoints(&config).is_none());
    }
}
