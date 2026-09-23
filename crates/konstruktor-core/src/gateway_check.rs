//! Whether every service answers on every address the hub advertises.
//!
//! The per-service health dots ask through `localhost`, which proves the services and the
//! gateway work, but says nothing about the addresses clients are actually handed: a LAN
//! address the gateway is not published on, a port the firewall eats, a mesh node the
//! gateway is not routed through. So this asks the same health path on each advertised
//! alias — every alias the coordination server gives out, not just one.
//!
//! Only from where this machine stands. An address this machine cannot reach — somebody
//! else's network, a tailnet this machine is not on — is reported as *not reachable from
//! here*, which is not a failure of the hub; it is simply not something this check can
//! answer. What it can answer, it answers for every service.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::hub::{scheme_of, HubConfig};
use crate::connect::manifest::advertised_port;

/// How long an address gets to accept a connection before it counts as not reachable
/// from here. Short: a LAN address either answers at once or is somewhere else.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long each service gets to answer through the gateway.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// One service, asked through one alias.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceProbe {
    /// The service's path on the gateway — `mikro`, `rekuest`.
    pub service: String,
    pub url: String,
    /// The HTTP status it answered with, when it answered at all.
    pub status: Option<u16>,
    pub healthy: bool,
    /// Why not, for a person: a status, a timeout, a refused connection.
    pub detail: Option<String>,
}

/// One advertised address, and what every service said through it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasProbe {
    pub host: String,
    pub port: u16,
    /// `advertised` for an address the manifest carries, `mesh` for the tailnet node.
    pub kind: String,
    /// Whether this machine could open a connection to it at all. When false, `services`
    /// is empty and `detail` says why: that is a limit of where the check runs, not a
    /// verdict on the hub.
    pub reachable: bool,
    pub detail: Option<String>,
    pub services: Vec<ServiceProbe>,
}

/// Everything, for every alias this hub advertises.
pub async fn check(dir: &Path, config: &HubConfig) -> Vec<AliasProbe> {
    let scheme = scheme_of(config);
    // What to ask, and where under the gateway: each service's health check, and the
    // S3 datalayer's — the same challenge the manifest hands the coordination server.
    let mut hosts: Vec<(String, String)> = config
        .enabled_services()
        .into_iter()
        .map(|id| {
            let host = config.service(id).host.clone();
            let path = format!("{host}/{}?format=json", crate::health::HEALTH_PATH);
            (host, path)
        })
        .collect();
    if config.minio.enabled {
        hosts.push(("s3".into(), crate::generate::caddy::S3_CHALLENGE.into()));
    }

    let mut aliases: Vec<(String, u16, &'static str)> = crate::credentials::read_credentials(dir)
        .map(|c| c.advertised_hosts)
        .unwrap_or_default()
        .into_iter()
        .map(|h| (h.host, advertised_port(config), "advertised"))
        .collect();

    // The tailnet node answers on the gateway's own port: on the mesh, Caddy is reached
    // inside the sidecar's namespace, where nothing is mapped.
    if let Some(name) = mesh_name(dir, config).await {
        let port = if config.gateway.ssl { 443 } else { 80 };
        aliases.push((name, port, "mesh"));
    }

    let mut seen = std::collections::HashSet::new();
    aliases.retain(|(host, port, _)| seen.insert((host.clone(), *port)));

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        // Self-signed and not-yet-issued certificates are the gateway's business; this
        // asks whether the service answers, not whether the certificate is right.
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap_or_default();

    let mut probes = Vec::new();
    for (host, port, kind) in aliases {
        probes.push(probe_alias(&client, scheme, &host, port, kind, &hosts).await);
    }
    probes
}

async fn probe_alias(
    client: &reqwest::Client,
    scheme: &str,
    host: &str,
    port: u16,
    kind: &str,
    services: &[(String, String)],
) -> AliasProbe {
    let mut alias = AliasProbe {
        host: host.to_string(),
        port,
        kind: kind.to_string(),
        reachable: false,
        detail: None,
        services: Vec::new(),
    };

    if let Err(why) = can_connect(host, port).await {
        alias.detail = Some(why);
        return alias;
    }
    alias.reachable = true;

    // Concurrently: one at a time would cost a timeout per service on a broken alias.
    let tasks: Vec<_> = services
        .iter()
        .map(|(service, path)| {
            let client = client.clone();
            let service = service.clone();
            let url = format!("{scheme}://{}:{port}/{path}", bracket(host));
            tokio::spawn(async move { ask(&client, service, url).await })
        })
        .collect();
    for task in tasks {
        if let Ok(probe) = task.await {
            alias.services.push(probe);
        }
    }
    alias
}

async fn ask(client: &reqwest::Client, service: String, url: String) -> ServiceProbe {
    match client.get(&url).send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let healthy = response.status().is_success();
            ServiceProbe {
                service,
                url,
                status: Some(status),
                healthy,
                detail: (!healthy).then(|| describe_status(status)),
            }
        }
        Err(error) => ServiceProbe {
            service,
            url,
            status: None,
            healthy: false,
            detail: Some(if error.is_timeout() {
                "no answer in time".into()
            } else {
                "the connection failed".into()
            }),
        },
    }
}

/// A 502 from the gateway is the one worth explaining: Caddy is up, the service is not
/// where it looked — which is the failure this check exists to find.
fn describe_status(status: u16) -> String {
    match status {
        502 | 503 | 504 => format!("{status} — the gateway could not reach the service"),
        404 => "404 — the gateway does not route this path".into(),
        other => format!("answered {other}"),
    }
}

/// An IPv6 literal needs brackets in a URL.
fn bracket(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

async fn can_connect(host: &str, port: u16) -> Result<(), String> {
    let target = format!("{}:{port}", bracket(host));
    let addresses: Vec<std::net::SocketAddr> = match tokio::net::lookup_host(&target).await {
        Ok(found) => found.collect(),
        Err(_) => return Err("does not resolve from this machine".into()),
    };
    if addresses.is_empty() {
        return Err("does not resolve from this machine".into());
    }
    for address in addresses {
        if let Ok(Ok(_)) =
            tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect(address)).await
        {
            return Ok(());
        }
    }
    Err("not reachable from this machine".into())
}

/// The tailnet node's full name, as the sidecar knows it.
async fn mesh_name(dir: &Path, config: &HubConfig) -> Option<String> {
    crate::hubhealth::from_sidecar(dir, config).await?.hostname
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gateway_that_cannot_find_the_service_is_named_as_such() {
        assert!(describe_status(502).contains("could not reach the service"));
        assert!(describe_status(404).contains("does not route"));
    }

    #[test]
    fn ipv6_literals_are_bracketed() {
        assert_eq!(bracket("fd7a::5"), "[fd7a::5]");
        assert_eq!(bracket("10.0.0.4"), "10.0.0.4");
        assert_eq!(bracket("hub.local"), "hub.local");
    }

    /// Nothing is listening on a port the test just freed, so the address counts as not
    /// reachable from here — without any service being asked anything.
    #[tokio::test]
    async fn an_unreachable_alias_asks_no_service() {
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let alias = probe_alias(
            &reqwest::Client::new(),
            "http",
            "127.0.0.1",
            port,
            "advertised",
            &[("mikro".to_string(), "mikro/ht?format=json".to_string())],
        )
        .await;
        assert!(!alias.reachable);
        assert!(alias.services.is_empty());
        assert_eq!(alias.detail.as_deref(), Some("not reachable from this machine"));
    }
}
