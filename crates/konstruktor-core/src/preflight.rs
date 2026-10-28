//! What has to be true of this machine before a public site is created on it: its names
//! point here, and the ports it is reached on are not somebody else's.
//!
//! Both are checked before anything is written, because neither fails where it is made.
//! A name that points elsewhere fails at the CA, minutes later, as an ACME error in the
//! gateway's log — and Let's Encrypt counts every failed validation against the name. A port
//! that is taken fails at `compose up`, after the folder was written and registered.

use std::net::IpAddr;
use std::time::Duration;

use serde::Serialize;

/// How long one name gets to resolve.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

/// What one name was found to resolve to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NameFinding {
    pub name: String,
    pub addresses: Vec<IpAddr>,
    /// Whether one of them is this machine's: an address on one of its interfaces, or the
    /// public address it was said to be reached at.
    pub points_here: bool,
}

/// Resolves each of `names` and says whether it points at one of `ours`.
pub async fn resolve_names(names: &[String], ours: &[IpAddr]) -> Vec<NameFinding> {
    let lookups = names.iter().map(|name| {
        let name = name.clone();
        async move {
            let host = name.clone();
            let addresses = tokio::time::timeout(
                LOOKUP_TIMEOUT,
                tokio::task::spawn_blocking(move || {
                    dns_lookup::lookup_host(&host).map(|found| found.collect::<Vec<IpAddr>>())
                }),
            )
            .await
            .ok()
            .and_then(Result::ok)
            .and_then(Result::ok)
            .unwrap_or_default();
            judge(name, addresses, ours)
        }
    });
    futures_util::future::join_all(lookups).await
}

fn judge(name: String, mut addresses: Vec<IpAddr>, ours: &[IpAddr]) -> NameFinding {
    addresses.sort();
    addresses.dedup();
    let points_here = addresses.iter().any(|address| ours.contains(address));
    NameFinding {
        name,
        addresses,
        points_here,
    }
}

/// This machine's addresses: every interface's, IPv4 and IPv6.
pub fn local_addresses() -> Vec<IpAddr> {
    use network_interface::{NetworkInterface, NetworkInterfaceConfig};
    NetworkInterface::show()
        .unwrap_or_default()
        .iter()
        .flat_map(|interface| interface.addr.iter().map(|addr| addr.ip()))
        .collect()
}

/// What is wrong with `findings`, in words that say what to do. `None` when every name
/// points here.
pub fn describe_names(findings: &[NameFinding]) -> Option<String> {
    let wrong: Vec<String> = findings
        .iter()
        .filter(|finding| !finding.points_here)
        .map(|finding| {
            if finding.addresses.is_empty() {
                format!("{} does not resolve", finding.name)
            } else {
                format!(
                    "{} resolves to {}, which is not this machine",
                    finding.name,
                    finding
                        .addresses
                        .iter()
                        .map(IpAddr::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        })
        .collect();
    if wrong.is_empty() {
        return None;
    }
    Some(format!(
        "{}. Point {} at this machine's public address first (an A or AAAA record), and \
         make sure ports 80 and 443 reach it. Behind a NAT or a load balancer this machine \
         cannot see its own public address: say it with `--public-ip`, or check by hand and \
         pass `--skip-dns-check`.",
        wrong.join("; "),
        if wrong.len() == 1 { "it" } else { "them" }
    ))
}

/// Of `ports`, the ones something on this machine already listens on.
pub async fn busy_ports(ports: &[u16]) -> Vec<u16> {
    let mut busy = Vec::new();
    for &port in ports {
        let answered = tokio::time::timeout(
            Duration::from_millis(500),
            tokio::net::TcpStream::connect(("127.0.0.1", port)),
        )
        .await
        .is_ok_and(|connected| connected.is_ok());
        if answered {
            busy.push(port);
        }
    }
    busy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn a_name_points_here_when_one_of_its_addresses_is_ours() {
        let ours = [ip("203.0.113.7"), ip("10.0.0.4")];
        let finding = judge(
            "coord.example.org".into(),
            vec![ip("203.0.113.7"), ip("2001:db8::1")],
            &ours,
        );
        assert!(finding.points_here);
        assert_eq!(describe_names(&[finding]), None);
    }

    #[test]
    fn a_name_that_points_elsewhere_or_nowhere_is_said_to() {
        let ours = [ip("203.0.113.7")];
        let elsewhere = judge("coord.example.org".into(), vec![ip("198.51.100.1")], &ours);
        let nowhere = judge("mesh.example.org".into(), Vec::new(), &ours);
        let said = describe_names(&[elsewhere, nowhere]).unwrap();
        assert!(
            said.contains("coord.example.org resolves to 198.51.100.1"),
            "{said}"
        );
        assert!(said.contains("mesh.example.org does not resolve"), "{said}");
        assert!(said.contains("--skip-dns-check"), "{said}");
    }

    #[tokio::test]
    async fn a_port_something_listens_on_is_busy() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        assert_eq!(busy_ports(&[port]).await, vec![port]);
        drop(listener);
    }
}
