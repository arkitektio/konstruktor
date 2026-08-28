use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Whether the outside world can actually get to this hub.
///
/// Two questions, kept apart on purpose, because one is cheap and one is not and they are
/// very easy to confuse:
///
/// * **Egress identity** — what address the internet sees this machine as. One request,
///   answered before anything is running, and it says *nothing* about whether a port is
///   open.
/// * **Port reachability** — whether something outside connected back to `host:port` and
///   got an answer. The real question, and the only one that may set `public` on an alias.
///
/// A green tick on the first read as the second is the failure mode to design against, so
/// nothing here ever collapses them into one boolean.

/// How long either check gets before it is written off as unknown.
///
/// Short on purpose: this decorates the address step, it does not gate it. Not knowing is
/// a perfectly good answer and must never be slower than reading the page.
const CHECK_TIMEOUT: Duration = Duration::from_secs(4);

/// What the internet says this machine's address is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EgressIdentity {
    pub address: String,
}

/// The result of asking something outside to connect back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "result")]
pub enum ProbeResult {
    /// Something answered. Any HTTP status counts — even a gateway error proves the
    /// socket was reached, which is the entire question.
    Reachable { status: u16 },
    /// Nothing answered.
    Unreachable { reason: String },
    /// No prober is configured, so nobody asked. Never rendered as a failure.
    NotChecked,
}

#[derive(Debug, thiserror::Error)]
pub enum ReachabilityError {
    #[error("Could not ask {endpoint}: {source}")]
    Unreachable {
        endpoint: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("{endpoint} answered {status}")]
    Refused { endpoint: String, status: u16 },
    #[error("{endpoint} did not answer with an address")]
    NotAnAddress { endpoint: String },
}

/// A client with a deadline on it.
///
/// The rest of this crate builds a bare `reqwest::Client` per call site, none of them with
/// a timeout. These calls are decoration, so they get one.
fn client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder().timeout(CHECK_TIMEOUT).build()
}

/// Asks an echo endpoint what address this machine appears to come from.
///
/// The endpoint is whatever the user configured. This is the only request konstruktor
/// makes to a host the user did not name as their coordination server, which is why it is
/// off unless somebody turns it on: it tells that host this machine's IP.
///
/// Accepts either a bare address in the body or a JSON object with an `ip`, `origin` or
/// `address` field, because the common echo services disagree about which.
pub async fn egress_identity(endpoint: &str) -> Result<EgressIdentity, ReachabilityError> {
    let endpoint = endpoint.trim();
    let response = client()
        .and_then(|c| Ok(c.get(endpoint)))
        .map_err(|source| ReachabilityError::Unreachable {
            endpoint: endpoint.to_string(),
            source,
        })?
        .send()
        .await
        .map_err(|source| ReachabilityError::Unreachable {
            endpoint: endpoint.to_string(),
            source,
        })?;

    let status = response.status();
    if !status.is_success() {
        return Err(ReachabilityError::Refused {
            endpoint: endpoint.to_string(),
            status: status.as_u16(),
        });
    }

    let body = response
        .text()
        .await
        .map_err(|source| ReachabilityError::Unreachable {
            endpoint: endpoint.to_string(),
            source,
        })?;

    address_from_echo(&body)
        .map(|address| EgressIdentity { address })
        .ok_or(ReachabilityError::NotAnAddress {
            endpoint: endpoint.to_string(),
        })
}

/// Pulls an address out of whatever an echo service answered with.
fn address_from_echo(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.parse::<std::net::IpAddr>().is_ok() {
        return Some(trimmed.to_string());
    }

    let json: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    ["ip", "origin", "address"]
        .into_iter()
        .find_map(|field| json.get(field).and_then(|v| v.as_str()))
        .map(str::trim)
        // `origin` is sometimes a comma-separated proxy chain; the first hop is us.
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| value.parse::<std::net::IpAddr>().is_ok())
        .map(str::to_string)
}

/// The URL an external prober should try for a given advertised host.
///
/// The gateway root, deliberately — not a health path. The manifest carries
/// `challenge: "ht"`, which looks like it names one, but nothing in this repository
/// defines what a challenge is or serves such a route, and the one comment that mentions
/// `/ht` disagrees with the code beneath it. Reachability does not need the distinction:
/// any answer at all, including an error from the proxy, proves the socket was reached.
pub fn probe_url(host: &str, port: u16, ssl: bool) -> String {
    let scheme = if ssl { "https" } else { "http" };
    // A v6 literal has to be bracketed before a port can be appended to it.
    let authority = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    format!("{scheme}://{authority}:{port}/")
}

/// Asks a prober to fetch `target` and tell us what happened.
///
/// The prober is a configured endpoint taking the target as a `url` query parameter. No
/// default ships: an external prober has to be something that will fetch an arbitrary URL
/// on request, and there is no service of that kind this app should point at without
/// being told to. Unconfigured means [`ProbeResult::NotChecked`], never a red cross.
pub async fn probe(prober: &str, target: &str) -> ProbeResult {
    let prober = prober.trim();
    if prober.is_empty() {
        return ProbeResult::NotChecked;
    }

    let separator = if prober.contains('?') { '&' } else { '?' };
    let url = format!("{prober}{separator}url={}", urlencode(target));

    let sent = match client() {
        Ok(client) => client.get(&url).send().await,
        Err(error) => {
            return ProbeResult::Unreachable {
                reason: error.to_string(),
            }
        }
    };

    match sent {
        // The prober reached us and is relaying what it got. Any status is a yes.
        Ok(response) if response.status().is_success() => {
            match response.json::<ProbeReport>().await {
                Ok(report) if report.reachable() => ProbeResult::Reachable {
                    status: report.status.unwrap_or(0),
                },
                Ok(report) => ProbeResult::Unreachable {
                    reason: report.error.unwrap_or_else(|| "no answer".to_string()),
                },
                Err(error) => ProbeResult::Unreachable {
                    reason: error.to_string(),
                },
            }
        }
        Ok(response) => ProbeResult::Unreachable {
            reason: format!("the prober answered {}", response.status().as_u16()),
        },
        Err(error) => ProbeResult::Unreachable {
            reason: error.to_string(),
        },
    }
}

/// What a prober is expected to answer with. Everything is optional, because a prober
/// this app does not ship cannot be held to a schema.
#[derive(Debug, Deserialize)]
struct ProbeReport {
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    reachable: Option<bool>,
    #[serde(default)]
    error: Option<String>,
}

impl ProbeReport {
    fn reachable(&self) -> bool {
        // An explicit answer wins; otherwise any status at all means the socket answered.
        self.reachable.unwrap_or(self.status.is_some())
    }
}

/// Percent-encodes the few characters that would otherwise break out of a query value.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_probe_url_for_the_gateway_root() {
        assert_eq!(
            probe_url("140.78.80.150", 80, false),
            "http://140.78.80.150:80/"
        );
        assert_eq!(
            probe_url("hub.example.org", 443, true),
            "https://hub.example.org:443/"
        );
    }

    /// A v6 literal needs brackets before a port, or the last group reads as the port.
    #[test]
    fn brackets_a_v6_literal() {
        assert_eq!(
            probe_url("2001:db8::1", 443, true),
            "https://[2001:db8::1]:443/"
        );
        assert_eq!(
            probe_url("[2001:db8::1]", 443, true),
            "https://[2001:db8::1]:443/"
        );
    }

    #[test]
    fn reads_an_address_out_of_whatever_the_echo_answered() {
        assert_eq!(
            address_from_echo("140.78.80.150\n").as_deref(),
            Some("140.78.80.150")
        );
        assert_eq!(
            address_from_echo(r#"{"ip":"140.78.80.150"}"#).as_deref(),
            Some("140.78.80.150")
        );
        assert_eq!(
            address_from_echo(r#"{"origin":"140.78.80.150, 10.0.0.1"}"#).as_deref(),
            Some("140.78.80.150")
        );
        assert_eq!(address_from_echo("<html>no</html>"), None);
        assert_eq!(address_from_echo(r#"{"ip":"not-an-address"}"#), None);
    }

    #[tokio::test]
    async fn an_unconfigured_prober_checks_nothing() {
        assert_eq!(
            probe("", "http://10.0.0.4:80/").await,
            ProbeResult::NotChecked
        );
        assert_eq!(
            probe("   ", "http://10.0.0.4:80/").await,
            ProbeResult::NotChecked
        );
    }

    #[test]
    fn encodes_the_target_into_the_query() {
        assert_eq!(
            urlencode("http://10.0.0.4:80/"),
            "http%3A%2F%2F10.0.0.4%3A80%2F"
        );
    }

    /// Any status means the socket answered — a 502 from the gateway still proves the
    /// port is open, which is the only thing being asked.
    #[test]
    fn any_status_counts_as_reachable() {
        let answered = ProbeReport {
            status: Some(502),
            reachable: None,
            error: None,
        };
        assert!(answered.reachable());

        let nothing = ProbeReport {
            status: None,
            reachable: None,
            error: Some("timeout".into()),
        };
        assert!(!nothing.reachable());

        let explicit = ProbeReport {
            status: Some(200),
            reachable: Some(false),
            error: None,
        };
        assert!(!explicit.reachable());
    }
}
