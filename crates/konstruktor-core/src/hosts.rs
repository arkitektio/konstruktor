use std::collections::HashSet;
use std::net::IpAddr;
use std::time::Duration;

use dns_lookup::{lookup_addr, lookup_host};
use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
use serde::{Deserialize, Serialize};

/// Which of this machine's addresses are worth advertising to a coordination server.
///
/// The rules mirror `arkitekt_next/server/connect.py :: discover_host_candidates`,
/// because the point is to advertise addresses *other* machines can reach — not the ones
/// that only make sense from inside this host.
///
/// Nothing is silently discarded any more. An address that exists but is useless to a
/// peer is still reported, carrying `usable: false` and the reason why, because "the
/// picker did not offer my docker bridge" is a question somebody will ask, and the honest
/// answer belongs next to the address rather than in this file.

/// Interfaces that only exist for containers and virtual machines.
const VIRTUAL_PREFIXES: [&str; 13] = [
    "docker", "br-", "veth", "virbr", "vnet", "vmnet", "vboxnet", "cni", "cbr", "flannel",
    "cali", "kube", "nerdctl",
];

/// Interfaces belonging to a tailnet. `utun` is deliberately absent: it is generic on
/// macOS, and the CGNAT range below is what identifies a tailnet address there.
const MESH_PREFIXES: [&str; 2] = ["tailscale", "ionscale"];

/// How long every name lookup in one discovery pass gets, in total.
///
/// The budget is shared rather than per-address on purpose. Lookups are all in flight at
/// once, so the pass costs about as much as its slowest answer — but a machine with a
/// dozen interfaces and a dead resolver must not be able to stall the wizard's address
/// step for a dozen timeouts in a row.
const LOOKUP_BUDGET: Duration = Duration::from_secs(3);

/// One address found on this machine, with whatever DNS had to say about it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub name: String,
    /// The reverse-resolved name, or the address again when resolution failed.
    pub host: String,
    pub bind: String,
    pub broadcast: Option<String>,
    pub successfull_dns: bool,
    /// What `host` resolves to when looked up *forwards*, which is the only way to tell a
    /// name that points back here from one that points somewhere else entirely.
    #[serde(default)]
    pub host_resolves_to: Vec<String>,
}

/// What an address or a name is, in enough detail to decide how far it reaches.
///
/// Richer than the three kinds this used to have, and deliberately so: `alias_scope` maps
/// these onto the four values the coordination server accepts, and a category that
/// collapses too early — a tailnet address indistinguishable from a public one — is
/// exactly how the wrong scope ends up on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HostCategory {
    /// 127.0.0.0/8 or ::1 — this machine, and nowhere else.
    Loopback,
    /// RFC1918 or a unique-local v6 address: the LAN.
    Private,
    /// A tailnet address belonging to the tailnet this hub is on.
    Mesh,
    /// A tailnet address that is not this hub's tailnet — or one we cannot show belongs
    /// to it. Somebody else's tailscale, as far as this hub is concerned.
    OtherMesh,
    /// Routable from the internet, firewalls permitting.
    Public,
    /// A container or VM bridge. Exists; means nothing to a peer.
    Virtual,
    /// 169.254/16 or fe80::/10.
    LinkLocal,
    /// A `*.local` name, resolvable by whoever speaks mDNS on this network.
    MdnsName,
    /// A name with no dot in it. Only ever resolves for someone sharing our search domain.
    BareHostname,
    /// A dotted name we could not confirm points back here.
    Fqdn,
    /// A dotted name that forward-resolves to one of this machine's own addresses.
    VerifiedFqdn,
}

/// Why a candidate is offered but not worth advertising.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnusableReason {
    VirtualInterface,
    LinkLocal,
    Unspecified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostCandidate {
    pub value: String,
    pub kind: HostCategory,
    pub interface: String,
    /// Pre-ticked by the presets. Implies `usable`.
    pub recommended: bool,
    /// False for addresses that exist but cannot help a peer reach this hub.
    pub usable: bool,
    pub unusable_reason: Option<UnusableReason>,
    /// What this address is, in words. The frontend renders this rather than keeping its
    /// own copy of the same sentences.
    pub summary: String,
}

/// What is known about the tailnet this hub belongs to.
///
/// A machine can be on several tailnets at once — most developer laptops running this are
/// already on a personal one — and an address on the wrong tailnet is reachable by
/// exactly nobody the coordination server knows about. Telling them apart needs to know
/// which tailnet is *ours*, and that is not something an address can be asked.
///
/// Two signals, both weak on their own and both absent often enough that "unknown" has to
/// be the safe answer:
///
/// * `domain` — the MagicDNS suffix, `hyena-sole.ts.net` and the like. The coordination
///   server is the only thing that really knows this, so it belongs in its well-known;
///   [`crate::connect::wellknown::WellKnownFakts::mesh_domain`] reads it from there when
///   a server declares it.
/// * `hostname` — the name this hub takes on the tailnet, out of its own mesh config.
///   Enough to recognise the hub's own node once it has joined, even with no domain.
///
/// With neither, every tailnet address is [`HostCategory::OtherMesh`]. That is not a
/// failure: during the wizard the hub has not joined anything yet, so a tailnet address
/// on this machine genuinely is somebody else's.
#[derive(Debug, Clone, Default)]
pub struct KnownMesh {
    pub domain: Option<String>,
    pub hostname: Option<String>,
}

impl KnownMesh {
    /// Whether a name is on this hub's tailnet.
    pub fn claims(&self, name: &str) -> bool {
        let lower = name.trim().to_ascii_lowercase();
        if lower.is_empty() {
            return false;
        }

        if let Some(domain) = &self.domain {
            let domain = domain.trim().trim_start_matches('.').to_ascii_lowercase();
            if !domain.is_empty() && (lower == domain || lower.ends_with(&format!(".{domain}"))) {
                return true;
            }
        }

        // No suffix to go on, but the hub's own node answers to a name we chose. Only the
        // first label: `mylab.example.ts.net` is ours, `mylab-backup.…` is not.
        if let Some(hostname) = &self.hostname {
            let hostname = hostname.trim().to_ascii_lowercase();
            if !hostname.is_empty() && lower.split('.').next() == Some(hostname.as_str()) {
                return true;
            }
        }

        false
    }
}

/// How far an address reaches, before the interface it was found on is considered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressClass {
    Unspecified,
    Loopback,
    LinkLocal,
    Private,
    /// RFC 6598 shared address space, 100.64.0.0/10 — a tailnet, here.
    Cgnat,
    Global,
}

/// What sort of name something is, judged on its spelling alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameClass {
    /// An IP literal wearing a name's clothes.
    NotAName,
    /// Ends in `.ts.net`.
    MeshName,
    Mdns,
    Bare,
    Dotted,
}

pub fn is_virtual_interface(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    VIRTUAL_PREFIXES.iter().any(|p| lower.starts_with(p))
}

pub fn is_mesh_interface(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    MESH_PREFIXES.iter().any(|p| lower.starts_with(p))
}

pub fn is_loopback(address: &str) -> bool {
    classify_address(address) == AddressClass::Loopback
}

pub fn is_link_local(address: &str) -> bool {
    classify_address(address) == AddressClass::LinkLocal
}

pub fn is_private(address: &str) -> bool {
    classify_address(address) == AddressClass::Private
}

/// RFC 6598 shared address space: `100.64.0.0` through `100.127.255.255`.
///
/// The second octet is the whole point — a `/10`, not a `/8`. `100.128.0.0` is ordinary
/// public space and must not be mistaken for a tailnet.
pub fn is_cgnat(address: &str) -> bool {
    classify_address(address) == AddressClass::Cgnat
}

/// Classifies a bare address string. Anything unparseable is `Global`, on the principle
/// that a hand-written host we cannot make sense of is treated as the widest thing it
/// could be rather than quietly downgraded.
pub fn classify_address(address: &str) -> AddressClass {
    let Ok(ip) = address.parse::<IpAddr>() else {
        return AddressClass::Global;
    };
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_unspecified() {
                AddressClass::Unspecified
            } else if v4.is_loopback() {
                AddressClass::Loopback
            } else if v4.is_link_local() {
                AddressClass::LinkLocal
            } else if v4.is_private() {
                AddressClass::Private
            } else if v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1]) {
                AddressClass::Cgnat
            } else {
                AddressClass::Global
            }
        }
        IpAddr::V6(v6) => {
            if v6.is_unspecified() {
                AddressClass::Unspecified
            } else if v6.is_loopback() {
                AddressClass::Loopback
            } else if (v6.segments()[0] & 0xffc0) == 0xfe80 {
                AddressClass::LinkLocal
            } else if (v6.segments()[0] & 0xfe00) == 0xfc00 {
                // Unique-local, fc00::/7 — the v6 equivalent of RFC1918.
                AddressClass::Private
            } else {
                AddressClass::Global
            }
        }
    }
}

/// Classifies a name on spelling alone. No DNS, so this costs nothing.
///
/// The first line is load-bearing. A reverse lookup that merely echoes the address back
/// still reports success, and an address masquerading as a hostname helps nobody — it
/// would be offered a second time, as a "name", and scoped by the wrong rules.
pub fn classify_name(name: &str) -> NameClass {
    if name.parse::<IpAddr>().is_ok() {
        return NameClass::NotAName;
    }
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".ts.net") {
        NameClass::MeshName
    } else if lower.ends_with(".local") {
        NameClass::Mdns
    } else if !lower.contains('.') {
        NameClass::Bare
    } else {
        NameClass::Dotted
    }
}

/// The category of a host given as text and nothing else — no interface, no DNS.
///
/// This is what `--host` gets. It cannot reach `VerifiedFqdn`, because verifying costs a
/// lookup the CLI has not made, and a tailnet address comes out [`HostCategory::OtherMesh`]
/// rather than `Mesh`: attributing one to *this hub's* tailnet takes a
/// [`KnownMesh`], and somebody spelling out `--host` is telling us where to reach the hub,
/// not which tailnet it is on. See [`classify_host_with`] when the tailnet is known.
pub fn classify_host(host: &str) -> HostCategory {
    classify_host_with(host, &KnownMesh::default())
}

/// [`classify_host`], for a caller that knows which tailnet is this hub's.
pub fn classify_host_with(host: &str, mesh: &KnownMesh) -> HostCategory {
    let category = classify_host_unattributed(host);
    if category == HostCategory::OtherMesh && mesh.claims(host) {
        return HostCategory::Mesh;
    }
    category
}

fn classify_host_unattributed(host: &str) -> HostCategory {
    match classify_name(host) {
        NameClass::NotAName => match classify_address(host) {
            AddressClass::Loopback | AddressClass::Unspecified => HostCategory::Loopback,
            AddressClass::LinkLocal => HostCategory::LinkLocal,
            AddressClass::Private => HostCategory::Private,
            AddressClass::Cgnat => HostCategory::OtherMesh,
            AddressClass::Global => HostCategory::Public,
        },
        NameClass::MeshName => HostCategory::OtherMesh,
        NameClass::Mdns => HostCategory::MdnsName,
        NameClass::Bare => HostCategory::BareHostname,
        NameClass::Dotted => HostCategory::Fqdn,
    }
}

pub fn describe_host_category(kind: HostCategory) -> &'static str {
    match kind {
        HostCategory::Loopback => "only reachable from this machine",
        HostCategory::Private => "reachable on the local network",
        HostCategory::Mesh => "reachable over the mesh",
        HostCategory::OtherMesh => "on a tailnet, but not this hub's — only machines on that tailnet",
        HostCategory::Public => "reachable from outside this network",
        HostCategory::Virtual => "a container bridge — nothing outside this machine can use it",
        HostCategory::LinkLocal => "a link-local address — it never leaves this cable",
        HostCategory::MdnsName => "an mDNS name, for machines on this network that speak it",
        HostCategory::BareHostname => "a bare name — it only resolves for machines sharing our DNS",
        HostCategory::Fqdn => "a name this machine resolves to, unconfirmed",
        HostCategory::VerifiedFqdn => "a name that resolves back to this machine",
    }
}

/// Whether a category is worth putting in front of a peer at all.
fn usable(kind: HostCategory) -> bool {
    !matches!(kind, HostCategory::Virtual | HostCategory::LinkLocal)
}

/// Whether a preset should tick this by default.
///
/// Loopback is usable but never recommended: it is a deliberate choice ("just this
/// machine"), not something to be handed out by accident.
///
/// No unconfirmed name is, either. A bare hostname is usually the `/etc/hosts` entry a
/// Linux box gives itself, which resolves to `127.0.1.1` — advertising that sends a peer
/// to its own loopback. They stay tickable for somebody who knows their DNS agrees; they
/// are never chosen on that person's behalf.
fn recommended(kind: HostCategory) -> bool {
    matches!(
        kind,
        HostCategory::Private
            | HostCategory::Mesh
            | HostCategory::Public
            | HostCategory::VerifiedFqdn
    )
}

fn unusable_reason(kind: HostCategory) -> Option<UnusableReason> {
    match kind {
        HostCategory::Virtual => Some(UnusableReason::VirtualInterface),
        HostCategory::LinkLocal => Some(UnusableReason::LinkLocal),
        _ => None,
    }
}

fn candidate(value: &str, kind: HostCategory, interface: &str) -> HostCandidate {
    HostCandidate {
        value: value.to_string(),
        kind,
        interface: interface.to_string(),
        recommended: recommended(kind),
        usable: usable(kind),
        unusable_reason: unusable_reason(kind),
        summary: describe_host_category(kind).to_string(),
    }
}

/// The category of one of this machine's own addresses, which — unlike a hand-given host
/// — comes with the interface it was found on.
fn category_for_binding(binding: &Binding, mesh: &KnownMesh) -> HostCategory {
    let class = classify_address(&binding.bind);

    // An address cannot say which tailnet it is on, so the name it reverse-resolved to is
    // the only evidence available. Unattributed means somebody else's tailnet.
    let tailnet = |binding: &Binding| {
        if mesh.claims(&binding.host) {
            HostCategory::Mesh
        } else {
            HostCategory::OtherMesh
        }
    };

    // A bridge address is useless to a peer whatever it looks like, but loopback and
    // link-local are described by what they *are*, which is more informative than the
    // interface they happen to sit on.
    match class {
        AddressClass::Loopback | AddressClass::Unspecified => HostCategory::Loopback,
        AddressClass::LinkLocal => HostCategory::LinkLocal,
        AddressClass::Cgnat => tailnet(binding),
        AddressClass::Private | AddressClass::Global => {
            if is_virtual_interface(&binding.name) {
                HostCategory::Virtual
            } else if is_mesh_interface(&binding.name) {
                tailnet(binding)
            } else if class == AddressClass::Private {
                HostCategory::Private
            } else {
                HostCategory::Public
            }
        }
    }
}

/// The category of the name a binding reverse-resolved to.
///
/// The address the name came from decides a good deal of this. A bridge's PTR record is
/// as useless as the bridge, and the name loopback resolves to is `localhost` — which is
/// a real answer to "just this machine" but emphatically not a network address, however
/// much it looks like a hostname.
fn category_for_name(
    binding: &Binding,
    local_addresses: &HashSet<&str>,
    mesh: &KnownMesh,
) -> Option<HostCategory> {
    if !binding.successfull_dns || binding.host.is_empty() {
        return None;
    }
    match category_for_binding(binding, mesh) {
        HostCategory::Virtual | HostCategory::LinkLocal => return None,
        HostCategory::Loopback => return Some(HostCategory::Loopback),
        // The address was already attributed; its name is on the same tailnet by
        // definition, whatever the name happens to be spelled like.
        attributed @ (HostCategory::Mesh | HostCategory::OtherMesh) => return Some(attributed),
        _ => {}
    }
    Some(match classify_name(&binding.host) {
        NameClass::NotAName => return None,
        NameClass::MeshName => {
            if mesh.claims(&binding.host) {
                HostCategory::Mesh
            } else {
                HostCategory::OtherMesh
            }
        }
        NameClass::Mdns => HostCategory::MdnsName,
        NameClass::Bare => HostCategory::BareHostname,
        NameClass::Dotted => {
            // Verified means it points *back here*. A name that resolves to somebody
            // else's address, or to loopback — which is what an /etc/hosts entry for the
            // machine's own hostname usually gives — is worse than no name at all.
            let points_here = binding
                .host_resolves_to
                .iter()
                .any(|address| local_addresses.contains(address.as_str()));
            if points_here {
                HostCategory::VerifiedFqdn
            } else {
                HostCategory::Fqdn
            }
        }
    })
}

/// Where a category sorts. Addresses before names, most reachable first, and everything
/// that cannot help a peer last.
fn sort_rank(kind: HostCategory) -> u8 {
    match kind {
        HostCategory::Public => 0,
        HostCategory::Private => 1,
        HostCategory::Mesh => 2,
        HostCategory::VerifiedFqdn => 3,
        HostCategory::Fqdn => 4,
        HostCategory::MdnsName => 5,
        HostCategory::BareHostname => 6,
        HostCategory::Loopback => 7,
        // After everything this hub can vouch for, before what it cannot use at all.
        HostCategory::OtherMesh => 8,
        HostCategory::Virtual => 9,
        HostCategory::LinkLocal => 10,
    }
}

/// Turns the raw address list into a de-duplicated, ordered set of candidates.
pub fn host_candidates(bindings: &[Binding], mesh: &KnownMesh) -> Vec<HostCandidate> {
    let local_addresses: HashSet<&str> = bindings.iter().map(|b| b.bind.as_str()).collect();

    let mut candidates: Vec<HostCandidate> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    let mut add = |candidate: HostCandidate, out: &mut Vec<HostCandidate>| {
        if !seen.contains(&candidate.value) {
            seen.push(candidate.value.clone());
            out.push(candidate);
        }
    };

    for binding in bindings {
        if binding.bind.is_empty() {
            continue;
        }

        add(
            candidate(&binding.bind, category_for_binding(binding, mesh), &binding.name),
            &mut candidates,
        );

        if let Some(kind) = category_for_name(binding, &local_addresses, mesh) {
            add(candidate(&binding.host, kind, &binding.name), &mut candidates);
        }
    }

    // Stable: within a category, discovery order is preserved.
    candidates.sort_by_key(|c| sort_rank(c.kind));
    candidates
}

/// The three answers to "how far should this hub reach".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReachPresetId {
    LocalOnly,
    ThisNetwork,
    Public,
}

/// A preset, resolved against what this machine actually has.
///
/// The core decides membership rather than the frontend, so the wizard and `--reach`
/// cannot drift apart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReachPreset {
    pub id: ReachPresetId,
    pub label: String,
    pub description: String,
    /// The candidate values this preset selects, in candidate order.
    pub values: Vec<String>,
}

impl ReachPresetId {
    pub fn label(self) -> &'static str {
        match self {
            ReachPresetId::LocalOnly => "Local only",
            ReachPresetId::ThisNetwork => "This network",
            ReachPresetId::Public => "Public",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            ReachPresetId::LocalOnly => "Only this machine reaches the hub.",
            ReachPresetId::ThisNetwork => "Machines on your network, and anything on the mesh.",
            ReachPresetId::Public => "Anything that can reach this machine from the internet.",
        }
    }

    /// Which categories this preset takes.
    ///
    /// Nested on purpose: each wider answer keeps everything the narrower one had. A hub
    /// advertised to the internet is still worth reaching from its own LAN, and the
    /// narrower aliases carry a narrower scope, so they cost a client nothing.
    ///
    /// [`HostCategory::OtherMesh`] is in none of them. A tailnet this hub is not on is
    /// reachable by the machines on *that* tailnet and nobody else, which may well be
    /// what somebody wants — but it is not something to choose on their behalf.
    pub fn accepts(self, kind: HostCategory) -> bool {
        let local = matches!(kind, HostCategory::Loopback);
        let network = local
            || matches!(
                kind,
                HostCategory::Private | HostCategory::Mesh | HostCategory::MdnsName
            );
        match self {
            ReachPresetId::LocalOnly => local,
            ReachPresetId::ThisNetwork => network,
            ReachPresetId::Public => {
                network || matches!(kind, HostCategory::Public | HostCategory::VerifiedFqdn)
            }
        }
    }
}

/// Resolves every preset against a candidate list.
pub fn reach_presets(candidates: &[HostCandidate]) -> Vec<ReachPreset> {
    [
        ReachPresetId::LocalOnly,
        ReachPresetId::ThisNetwork,
        ReachPresetId::Public,
    ]
    .into_iter()
    .map(|id| ReachPreset {
        id,
        label: id.label().to_string(),
        description: id.description().to_string(),
        values: candidates
            .iter()
            .filter(|c| c.usable && id.accepts(c.kind))
            .map(|c| c.value.clone())
            .collect(),
    })
    .collect()
}

/// Everything the address step needs, in one answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostDiscovery {
    pub candidates: Vec<HostCandidate>,
    pub presets: Vec<ReachPreset>,
}

/// Enumerates this machine's IPv4 addresses and works out what each one is.
///
/// IPv4 only, still. Enumerating v6 is trivial — `Addr::ip()` hands back an `IpAddr` —
/// but an advertised host reaches only the coordination server, and whether it brackets a
/// v6 literal when it builds `host:port` cannot be answered from this repository. An
/// unbracketed `2001:db8::1:443` is worse than no alias.
///
/// Resolution happens in two passes, both bounded. The reverse pass asks what each
/// address is called; the forward pass asks where those names actually point, which is
/// the only way to tell a name that comes back here from one that does not.
pub async fn bindings() -> Result<Vec<Binding>, String> {
    let interfaces = NetworkInterface::show().map_err(|e| e.to_string())?;

    let mut found = Vec::new();
    for itf in interfaces.iter() {
        for addr in itf.addr.iter() {
            let Addr::V4(v4) = addr else { continue };
            found.push((itf.name.clone(), v4.ip, v4.broadcast.map(|b| b.to_string())));
        }
    }

    // Every lookup is started before any is awaited, so the pass costs about as much as
    // its slowest answer rather than the sum of them all.
    let deadline = tokio::time::Instant::now() + LOOKUP_BUDGET;
    let reverse: Vec<_> = found
        .iter()
        .map(|(_, ip, _)| {
            let ip = *ip;
            tokio::task::spawn_blocking(move || lookup_addr(&ip.into()).ok())
        })
        .collect();

    let mut bindings = Vec::with_capacity(found.len());
    for ((name, ip, broadcast), job) in found.into_iter().zip(reverse) {
        let resolved = tokio::time::timeout_at(deadline, job)
            .await
            .ok()
            .and_then(Result::ok)
            .flatten();

        bindings.push(Binding {
            name,
            host: resolved.clone().unwrap_or_else(|| ip.to_string()),
            bind: ip.to_string(),
            broadcast,
            successfull_dns: resolved.is_some(),
            host_resolves_to: Vec::new(),
        });
    }

    verify_names(&mut bindings).await;
    Ok(bindings)
}

/// Fills in `host_resolves_to` for every binding whose name is worth checking.
///
/// Failure is not an error: an unverified name is still offered, just never promoted to
/// `VerifiedFqdn`, so a resolver that will not answer costs a little confidence rather
/// than a candidate.
async fn verify_names(bindings: &mut [Binding]) {
    let deadline = tokio::time::Instant::now() + LOOKUP_BUDGET;

    let jobs: Vec<_> = bindings
        .iter()
        .map(|binding| {
            let worth_checking = binding.successfull_dns
                && matches!(classify_name(&binding.host), NameClass::Dotted);
            worth_checking.then(|| {
                let host = binding.host.clone();
                tokio::task::spawn_blocking(move || lookup_host(&host).ok())
            })
        })
        .collect();

    for (binding, job) in bindings.iter_mut().zip(jobs) {
        let Some(job) = job else { continue };
        let resolved = tokio::time::timeout_at(deadline, job)
            .await
            .ok()
            .and_then(Result::ok)
            .flatten();

        if let Some(addresses) = resolved {
            binding.host_resolves_to = addresses.into_iter().map(|a| a.to_string()).collect();
        }
    }
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
            host_resolves_to: Vec::new(),
        }
    }

    /// Candidates as the wizard sees them: no tailnet known, because the hub has not
    /// joined one yet.
    fn unattributed(bindings: &[Binding]) -> Vec<HostCandidate> {
        host_candidates(bindings, &KnownMesh::default())
    }

    fn find<'a>(found: &'a [HostCandidate], value: &str) -> &'a HostCandidate {
        found
            .iter()
            .find(|c| c.value == value)
            .unwrap_or_else(|| panic!("expected a candidate for {value}, got {found:?}"))
    }

    #[test]
    fn classifies_private_ranges_the_way_the_cli_does() {
        assert!(is_private("10.0.0.4"));
        assert!(is_private("192.168.1.10"));
        assert!(is_private("172.16.0.1"));
        assert!(is_private("172.31.255.254"));
        assert!(!is_private("172.15.0.1"));
        assert!(!is_private("172.32.0.1"));
        assert!(!is_private("140.78.80.150"));
    }

    /// The range is a /10, so the second octet decides it. Getting this wrong is how a
    /// tailnet address ends up advertised to the whole internet.
    #[test]
    fn recognises_the_cgnat_range_by_its_second_octet() {
        assert!(!is_cgnat("100.63.255.255"));
        assert!(is_cgnat("100.64.0.0"));
        assert!(is_cgnat("100.116.108.106"));
        assert!(is_cgnat("100.127.255.255"));
        assert!(!is_cgnat("100.128.0.0"));
    }

    #[test]
    fn classifies_addresses_by_what_they_are() {
        assert_eq!(classify_address("0.0.0.0"), AddressClass::Unspecified);
        assert_eq!(classify_address("127.0.0.1"), AddressClass::Loopback);
        assert_eq!(classify_address("169.254.1.1"), AddressClass::LinkLocal);
        assert_eq!(classify_address("::1"), AddressClass::Loopback);
        assert_eq!(classify_address("fe80::1"), AddressClass::LinkLocal);
        assert_eq!(classify_address("fd00::1"), AddressClass::Private);
        assert_eq!(classify_address("2001:db8::1"), AddressClass::Global);
    }

    /// Everything that exists is reported. What used to be dropped is now flagged, so the
    /// picker can say *why* a docker bridge is not on offer.
    #[test]
    fn flags_what_only_works_from_this_machine_instead_of_hiding_it() {
        let found = unattributed(&[
            binding("docker0", "172.17.0.1", "172.17.0.1", false),
            binding("br-abc", "172.20.0.1", "172.20.0.1", false),
            binding("veth123", "10.1.2.3", "10.1.2.3", false),
            binding("lo", "127.0.0.1", "localhost", true),
            binding("wlan0", "169.254.1.1", "", false),
        ]);

        for bridge in ["172.17.0.1", "172.20.0.1", "10.1.2.3"] {
            let candidate = find(&found, bridge);
            assert_eq!(candidate.kind, HostCategory::Virtual);
            assert!(!candidate.usable);
            assert_eq!(
                candidate.unusable_reason,
                Some(UnusableReason::VirtualInterface)
            );
        }

        let link_local = find(&found, "169.254.1.1");
        assert_eq!(link_local.kind, HostCategory::LinkLocal);
        assert_eq!(link_local.unusable_reason, Some(UnusableReason::LinkLocal));

        // Loopback is a real answer to "just this machine" — offered, never assumed.
        let loopback = find(&found, "127.0.0.1");
        assert_eq!(loopback.kind, HostCategory::Loopback);
        assert!(loopback.usable);
        assert!(!loopback.recommended);
    }

    /// Splitting one name category into four does not disturb this: with no forward
    /// lookup, both names are unverified `Fqdn`, so the stable sort keeps them in
    /// discovery order behind the addresses.
    #[test]
    fn offers_public_addresses_before_private_ones_and_names_last() {
        let found = unattributed(&[
            binding("eth1", "10.0.0.4", "lab.internal", true),
            binding("eth0", "140.78.80.150", "server.example.org", true),
        ]);
        let values: Vec<&str> = found.iter().map(|c| c.value.as_str()).collect();
        assert_eq!(
            values,
            ["140.78.80.150", "10.0.0.4", "lab.internal", "server.example.org"]
        );
        assert!(found[0].recommended);
        assert!(!found[2].recommended);
    }

    /// A tailnet address is not a public one. It used to be classified as public, which
    /// advertised it to everybody and made it reachable by nobody.
    #[test]
    fn recognises_a_tailnet_address_however_it_is_found() {
        let by_range = unattributed(&[binding("eth0", "100.116.108.106", "", false)]);
        assert_eq!(by_range[0].kind, HostCategory::OtherMesh);

        let by_interface = unattributed(&[binding("tailscale0", "10.9.9.9", "", false)]);
        assert_eq!(by_interface[0].kind, HostCategory::OtherMesh);
    }

    /// The distinction the whole `KnownMesh` business exists for: this machine is on a
    /// personal tailnet, and the hub is being introduced to an organization's. Advertising
    /// the personal one as the hub's mesh offers the coordination server's peers an
    /// address none of them can route to.
    #[test]
    fn separates_this_hubs_tailnet_from_everybody_elses() {
        let bindings = [
            binding("tailscale0", "100.116.108.106", "laptop.hyena-sole.ts.net", true),
            binding("tailscale1", "100.70.0.9", "mylab.acme-org.ts.net", true),
        ];

        // Nothing known: both are somebody else's, which is the honest answer.
        let unknown = unattributed(&bindings);
        assert_eq!(find(&unknown, "100.116.108.106").kind, HostCategory::OtherMesh);
        assert_eq!(find(&unknown, "100.70.0.9").kind, HostCategory::OtherMesh);

        // The coordination server declares its tailnet: one of the two is ours now.
        let known = host_candidates(
            &bindings,
            &KnownMesh {
                domain: Some("acme-org.ts.net".to_string()),
                hostname: None,
            },
        );
        assert_eq!(find(&known, "100.116.108.106").kind, HostCategory::OtherMesh);
        assert_eq!(find(&known, "100.70.0.9").kind, HostCategory::Mesh);
        // The name is attributed with the address it was found on, not judged again.
        assert_eq!(find(&known, "mylab.acme-org.ts.net").kind, HostCategory::Mesh);
        assert_eq!(find(&known, "laptop.hyena-sole.ts.net").kind, HostCategory::OtherMesh);
    }

    /// No domain from the server, but the hub's own mesh config names its node — enough
    /// to recognise the hub's own address once it has joined.
    #[test]
    fn recognises_this_hubs_own_node_by_name() {
        let mesh = KnownMesh {
            domain: None,
            hostname: Some("mylab".to_string()),
        };
        assert!(mesh.claims("mylab.acme-org.ts.net"));
        // Only the first label: a different node with a similar name is not this hub.
        assert!(!mesh.claims("mylab-backup.acme-org.ts.net"));
        assert!(!mesh.claims("laptop.hyena-sole.ts.net"));
    }

    /// Somebody else's tailnet is offered — a lab where every client is on it is a real
    /// setup — but never chosen for them, because this hub cannot vouch for who is on it.
    #[test]
    fn never_puts_another_tailnet_in_a_preset() {
        let found = unattributed(&[
            binding("eth0", "10.0.0.4", "", false),
            binding("tailscale0", "100.116.108.106", "", false),
        ]);
        let other = find(&found, "100.116.108.106");
        assert_eq!(other.kind, HostCategory::OtherMesh);
        assert!(other.usable);
        assert!(!other.recommended);

        for preset in reach_presets(&found) {
            assert!(
                !preset.values.contains(&"100.116.108.106".to_string()),
                "{:?} offered another tailnet",
                preset.id
            );
        }
    }

    /// Reverse lookup that just echoes the address back reports success; without the
    /// literal check that would surface as a bogus "hostname" candidate.
    #[test]
    fn does_not_offer_an_address_as_a_hostname() {
        let found = unattributed(&[binding("eth0", "10.0.0.4", "10.0.0.4", true)]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, HostCategory::Private);
    }

    /// The same trap in v6 clothing: `is_ipv4` would have let this through, and a name
    /// with no dot in it would have been taken for a bare hostname.
    #[test]
    fn does_not_offer_a_v6_address_as_a_hostname() {
        assert_eq!(classify_name("2001:db8::1"), NameClass::NotAName);
        assert_eq!(classify_name("::1"), NameClass::NotAName);
    }

    #[test]
    fn grades_names_by_how_likely_they_are_to_resolve_elsewhere() {
        assert_eq!(classify_name("hub.local"), NameClass::Mdns);
        assert_eq!(classify_name("jhnnsrs-server"), NameClass::Bare);
        assert_eq!(classify_name("lab.example.org"), NameClass::Dotted);
        assert_eq!(classify_name("hub.tail1234.ts.net"), NameClass::MeshName);
    }

    /// A name is only promoted when it points back at us. The bare-hostname case that
    /// resolves to 127.0.1.1 — which is what most Linux boxes do — must not be.
    #[test]
    fn verifies_a_name_only_when_it_resolves_back_here() {
        let mut points_here = binding("eth0", "140.78.80.150", "lab.example.org", true);
        points_here.host_resolves_to = vec!["140.78.80.150".to_string()];
        let found = unattributed(&[points_here]);
        let name = find(&found, "lab.example.org");
        assert_eq!(name.kind, HostCategory::VerifiedFqdn);
        assert!(name.recommended);

        let mut points_elsewhere = binding("eth0", "140.78.80.150", "lab.example.org", true);
        points_elsewhere.host_resolves_to = vec!["127.0.1.1".to_string()];
        let found = unattributed(&[points_elsewhere]);
        let name = find(&found, "lab.example.org");
        assert_eq!(name.kind, HostCategory::Fqdn);
        assert!(!name.recommended);
    }

    /// Both found on a real machine, where the address step offered `localhost` and a
    /// virtual bridge's PTR record as things the *network* could reach.
    #[test]
    fn does_not_take_a_name_from_an_address_a_peer_cannot_use() {
        // `lo` resolves to `localhost`, which is a name, but not a network one.
        let loopback = unattributed(&[binding("lo", "127.0.0.1", "localhost", true)]);
        let name = find(&loopback, "localhost");
        assert_eq!(name.kind, HostCategory::Loopback);
        assert!(!name.recommended);

        // A bridge's reverse record is exactly as useless as the bridge itself.
        let bridge = unattributed(&[binding("virbr0", "192.168.122.1", "myhost", true)]);
        assert_eq!(bridge.len(), 1);
        assert_eq!(bridge[0].value, "192.168.122.1");
    }

    /// A bare hostname is usually the `/etc/hosts` entry pointing at 127.0.1.1. Offering
    /// it is fine; choosing it for somebody sends their peers to their own loopback.
    #[test]
    fn never_picks_an_unconfirmed_name_on_somebodys_behalf() {
        let found = unattributed(&[
            binding("eth0", "140.78.80.150", "myhost", true),
            binding("eth1", "10.0.0.4", "lab.example.org", true),
        ]);
        let presets = reach_presets(&found);

        for preset in &presets {
            assert!(!preset.values.contains(&"myhost".to_string()));
            assert!(!preset.values.contains(&"lab.example.org".to_string()));
        }
        // Still offered, so somebody who knows their DNS agrees can tick it.
        assert!(find(&found, "myhost").usable);
        assert!(find(&found, "lab.example.org").usable);
    }

    #[test]
    fn de_duplicates_an_address_seen_on_two_interfaces() {
        let found = unattributed(&[
            binding("eth0", "10.0.0.4", "", false),
            binding("eth1", "10.0.0.4", "", false),
        ]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].interface, "eth0");
    }

    #[test]
    fn every_recommended_candidate_is_usable() {
        let found = unattributed(&[
            binding("docker0", "172.17.0.1", "", false),
            binding("lo", "127.0.0.1", "localhost", true),
            binding("eth0", "140.78.80.150", "", false),
            binding("tailscale0", "100.116.108.106", "", false),
            binding("wlan0", "169.254.1.1", "", false),
        ]);
        assert!(found.iter().all(|c| !c.recommended || c.usable));
    }

    /// Each wider preset keeps everything the narrower one had, and none of them offers
    /// an address a peer cannot use.
    #[test]
    fn presets_nest_and_never_offer_something_unusable() {
        let found = unattributed(&[
            binding("lo", "127.0.0.1", "", false),
            binding("eth0", "140.78.80.150", "hub.example.org", true),
            binding("eth1", "10.0.0.4", "", false),
            binding("tailscale0", "100.116.108.106", "", false),
            binding("docker0", "172.17.0.1", "", false),
        ]);
        let presets = reach_presets(&found);

        let values = |id: ReachPresetId| -> Vec<String> {
            presets.iter().find(|p| p.id == id).unwrap().values.clone()
        };
        let local = values(ReachPresetId::LocalOnly);
        let network = values(ReachPresetId::ThisNetwork);
        let public = values(ReachPresetId::Public);

        assert!(local.iter().all(|v| network.contains(v)));
        assert!(network.iter().all(|v| public.contains(v)));

        assert_eq!(local, ["127.0.0.1"]);
        assert!(network.contains(&"10.0.0.4".to_string()));
        assert!(!network.contains(&"140.78.80.150".to_string()));
        assert!(public.contains(&"140.78.80.150".to_string()));

        for preset in &presets {
            assert!(!preset.values.contains(&"172.17.0.1".to_string()));
        }
    }

    /// What `--host` gets: no interface, no lookup, still a sensible answer.
    #[test]
    fn classifies_a_hand_given_host() {
        assert_eq!(classify_host("localhost"), HostCategory::BareHostname);
        assert_eq!(classify_host("127.0.0.1"), HostCategory::Loopback);
        assert_eq!(classify_host("10.0.0.4"), HostCategory::Private);
        // A tailnet, but not demonstrably this hub's — `--host` says where to reach the
        // hub, not which tailnet it belongs to.
        assert_eq!(classify_host("100.64.1.2"), HostCategory::OtherMesh);
        assert_eq!(classify_host("hub.tail1234.ts.net"), HostCategory::OtherMesh);

        // Told which tailnet is ours, the same host is attributed to it.
        let mesh = KnownMesh {
            domain: Some("tail1234.ts.net".to_string()),
            hostname: None,
        };
        assert_eq!(
            classify_host_with("hub.tail1234.ts.net", &mesh),
            HostCategory::Mesh
        );
        assert_eq!(
            classify_host_with("hub.someone-else.ts.net", &mesh),
            HostCategory::OtherMesh
        );
        assert_eq!(classify_host("140.78.80.150"), HostCategory::Public);
        assert_eq!(classify_host("lab.example.org"), HostCategory::Fqdn);
    }
}
