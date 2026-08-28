use std::time::Duration;

use dns_lookup::lookup_addr;
use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
use serde::{Deserialize, Serialize};

/// Which of this machine's addresses are worth advertising to a coordination server.
///
/// The rules mirror `arkitekt_next/server/connect.py :: discover_host_candidates`,
/// because the point is to advertise addresses *other* machines can reach — not the ones
/// that only make sense from inside this host.

/// Interfaces that only exist for containers and virtual machines.
const VIRTUAL_PREFIXES: [&str; 13] = [
    "docker", "br-", "veth", "virbr", "vnet", "vmnet", "vboxnet", "cni", "cbr", "flannel",
    "cali", "kube", "nerdctl",
];

/// One address found on this machine, with whatever reverse DNS had to say about it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub name: String,
    /// The reverse-resolved name, or the address again when resolution failed.
    pub host: String,
    pub bind: String,
    pub broadcast: Option<String>,
    pub successfull_dns: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostKind {
    Hostname,
    Private,
    Public,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostCandidate {
    pub value: String,
    pub kind: HostKind,
    pub interface: String,
    /// Pre-ticked in the picker: addresses are reliable, resolved names less so.
    pub recommended: bool,
}

pub fn is_virtual_interface(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    VIRTUAL_PREFIXES.iter().any(|p| lower.starts_with(p))
}

pub fn is_loopback(address: &str) -> bool {
    address.starts_with("127.") || address == "::1"
}

pub fn is_link_local(address: &str) -> bool {
    address.starts_with("169.254.") || address.to_ascii_lowercase().starts_with("fe80:")
}

pub fn is_private(address: &str) -> bool {
    if address.starts_with("10.") || address.starts_with("192.168.") {
        return true;
    }
    address
        .strip_prefix("172.")
        .and_then(|rest| rest.split('.').next())
        .and_then(|octet| octet.parse::<u8>().ok())
        .is_some_and(|octet| (16..=31).contains(&octet))
}

fn is_ipv4(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Turns the raw address list into a de-duplicated, ordered set of candidates: public
/// addresses first, then private ones, then resolved names.
pub fn host_candidates(bindings: &[Binding]) -> Vec<HostCandidate> {
    let mut candidates: Vec<HostCandidate> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    let add = |candidate: HostCandidate, seen: &mut Vec<String>, out: &mut Vec<HostCandidate>| {
        if !seen.contains(&candidate.value) {
            seen.push(candidate.value.clone());
            out.push(candidate);
        }
    };

    for binding in bindings {
        if is_virtual_interface(&binding.name) {
            continue;
        }
        let address = &binding.bind;
        if address.is_empty() || is_loopback(address) || is_link_local(address) {
            continue;
        }

        add(
            HostCandidate {
                value: address.clone(),
                kind: if is_private(address) {
                    HostKind::Private
                } else {
                    HostKind::Public
                },
                interface: binding.name.clone(),
                recommended: true,
            },
            &mut seen,
            &mut candidates,
        );

        // The reverse-resolved name, when there is a real one, is offered but not
        // pre-selected: it is only useful if the client's DNS agrees. The `is_ipv4` guard
        // is load-bearing — resolution that merely echoes the address back still reports
        // success, and an address masquerading as a hostname helps nobody.
        if binding.successfull_dns && !binding.host.is_empty() && !is_ipv4(&binding.host) {
            add(
                HostCandidate {
                    value: binding.host.clone(),
                    kind: HostKind::Hostname,
                    interface: binding.name.clone(),
                    recommended: false,
                },
                &mut seen,
                &mut candidates,
            );
        }
    }

    // Stable: within a kind, discovery order is preserved.
    candidates.sort_by_key(|c| match c.kind {
        HostKind::Public => 0,
        HostKind::Private => 1,
        HostKind::Hostname => 2,
    });
    candidates
}

pub fn describe_host_kind(kind: HostKind) -> &'static str {
    match kind {
        HostKind::Public => "reachable from outside this network",
        HostKind::Private => "reachable on the local network",
        HostKind::Hostname => "a name this machine resolves to",
    }
}

/// Enumerates this machine's IPv4 addresses, attempting reverse DNS on each.
///
/// Resolution is bounded: a nameserver that never answers would otherwise stall the
/// wizard's address step behind every interface in turn.
pub async fn bindings() -> Result<Vec<Binding>, String> {
    let interfaces = NetworkInterface::show().map_err(|e| e.to_string())?;
    let mut out = Vec::new();

    for itf in interfaces.iter() {
        for addr in itf.addr.iter() {
            let v4 = match addr {
                Addr::V4(v4) => *v4,
                _ => continue,
            };
            let ip = v4.ip;
            let broadcast = v4.broadcast.map(|b| b.to_string());

            let resolved = tokio::time::timeout(
                Duration::from_secs(3),
                tokio::task::spawn_blocking(move || lookup_addr(&ip.into()).ok()),
            )
            .await
            .ok()
            .and_then(Result::ok)
            .flatten();

            out.push(Binding {
                name: itf.name.clone(),
                host: resolved.clone().unwrap_or_else(|| ip.to_string()),
                bind: ip.to_string(),
                broadcast,
                successfull_dns: resolved.is_some(),
            });
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(name: &str, bind: &str, host: &str, dns: bool) -> Binding {
        Binding {
            name: name.to_string(),
            host: host.to_string(),
            bind: bind.to_string(),
            broadcast: None,
            successfull_dns: dns,
        }
    }

    #[test]
    fn classifies_private_ranges_the_way_the_cli_does() {
        assert!(is_private("10.0.0.4"));
        assert!(is_private("192.168.1.1"));
        assert!(is_private("172.16.0.1"));
        assert!(is_private("172.31.255.254"));
        // Just outside the /12.
        assert!(!is_private("172.15.0.1"));
        assert!(!is_private("172.32.0.1"));
        assert!(!is_private("140.78.80.150"));
    }

    #[test]
    fn skips_what_only_works_from_this_machine() {
        let found = host_candidates(&[
            binding("docker0", "172.17.0.1", "172.17.0.1", false),
            binding("br-abc", "172.20.0.1", "172.20.0.1", false),
            binding("veth123", "10.1.2.3", "10.1.2.3", false),
            binding("lo", "127.0.0.1", "localhost", true),
            binding("wlan0", "169.254.1.1", "", false),
        ]);
        assert!(found.is_empty(), "got {found:?}");
    }

    #[test]
    fn offers_public_addresses_before_private_ones_and_names_last() {
        let found = host_candidates(&[
            binding("eth1", "10.0.0.4", "lab.internal", true),
            binding("eth0", "140.78.80.150", "server.example.org", true),
        ]);
        // Public first, then private, then names — and *within* a kind the sort is
        // stable, so discovery order survives: eth1 was enumerated first, so its name is.
        let values: Vec<&str> = found.iter().map(|c| c.value.as_str()).collect();
        assert_eq!(
            values,
            ["140.78.80.150", "10.0.0.4", "lab.internal", "server.example.org"]
        );
        assert!(found[0].recommended);
        // Names are offered but never pre-ticked — they only work if the client's DNS
        // agrees with ours.
        assert!(!found[2].recommended);
    }

    /// Reverse lookup that just echoes the address back reports success; without the
    /// `is_ipv4` guard that would surface as a bogus "hostname" candidate.
    #[test]
    fn does_not_offer_an_address_as_a_hostname() {
        let found = host_candidates(&[binding("eth0", "10.0.0.4", "10.0.0.4", true)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, HostKind::Private);
    }

    #[test]
    fn de_duplicates_an_address_seen_on_two_interfaces() {
        let found = host_candidates(&[
            binding("eth0", "10.0.0.4", "10.0.0.4", false),
            binding("eth0:1", "10.0.0.4", "10.0.0.4", false),
        ]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].interface, "eth0");
    }
}
