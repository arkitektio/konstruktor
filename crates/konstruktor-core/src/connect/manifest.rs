use serde::{Deserialize, Serialize};

use crate::catalog::{ServiceId, HUB_SERVICE_ORDER};
use crate::config::hub::HubConfig;
use crate::hosts::HostCategory;

/// The hub manifest, as `deployments/next/mounts/lok` expects it.
///
/// Two shape gotchas worth knowing: a `ServiceManifest` carries no `name`, and
/// `HubManifest.identifier` must be unique inside the organization that accepts it.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub key: String,
    pub description: String,
}

fn entries(pairs: &[(&str, &str)]) -> Vec<ManifestEntry> {
    pairs
        .iter()
        .map(|(key, description)| ManifestEntry {
            key: (*key).to_string(),
            description: (*description).to_string(),
        })
        .collect()
}

fn roles_of(id: ServiceId) -> &'static [(&'static str, &'static str)] {
    match id {
        ServiceId::Rekuest => &[
            ("agent", "Can act as a workflow agent"),
            ("caller", "Can call remote procedures"),
            ("admin", "Full administrative access"),
        ],
        ServiceId::Mikro => &[
            ("admin", "Full administrative access"),
            ("user", "Standard user access"),
            ("viewer", "Read-only access to images"),
            ("uploader", "Can upload new images"),
        ],
        ServiceId::Fluss => &[
            ("admin", "Full administrative access"),
            ("user", "Standard user access"),
            ("designer", "Can design workflows"),
            ("viewer", "Read-only access"),
        ],
        ServiceId::Kabinet => &[
            ("admin", "Full administrative access"),
            ("deployer", "Can deploy containers"),
            ("user", "Standard user access"),
            ("viewer", "Read-only access"),
        ],
        ServiceId::Kraph => &[
            ("admin", "Full administrative access"),
            ("user", "Standard user access"),
            ("editor", "Can edit graph data"),
            ("viewer", "Read-only access"),
        ],
        ServiceId::Elektro => &[
            ("admin", "Full administrative access"),
            ("user", "Standard user access"),
            ("analyst", "Can analyze recordings"),
            ("viewer", "Read-only access"),
        ],
        ServiceId::Alpaka => &[
            ("admin", "Full administrative access"),
            ("user", "Standard user access"),
            ("modeler", "Can manage ML models"),
            ("viewer", "Read-only access"),
        ],
        ServiceId::Lovekit => &[],
    }
}

fn scopes_of(id: ServiceId) -> &'static [(&'static str, &'static str)] {
    match id {
        ServiceId::Rekuest => &[
            ("rekuest_agent", "Act as an agent"),
            ("rekuest_call", "Call other apps with rekuest"),
            ("read", "Read access to rekuest resources"),
            ("write", "Write access to rekuest resources"),
        ],
        ServiceId::Mikro => &[
            ("mikro_read", "Read images from the database"),
            ("mikro_write", "Write images to the database"),
            ("read_image", "Read image data"),
            ("read", "Generic read access"),
            ("write", "Generic write access"),
        ],
        ServiceId::Fluss => &[
            ("fluss_read", "Read workflow definitions"),
            ("fluss_write", "Create and modify workflows"),
            ("fluss_execute", "Execute workflows"),
            ("read", "Generic read access"),
            ("write", "Generic write access"),
        ],
        ServiceId::Kabinet => &[
            ("kabinet_add_repo", "Add repositories to the database"),
            ("kabinet_deploy", "Deploy containers"),
            ("kabinet_read", "Read container definitions"),
            ("read", "Generic read access"),
            ("write", "Generic write access"),
        ],
        ServiceId::Kraph => &[
            ("kraph_read", "Read graph data"),
            ("kraph_write", "Write graph data"),
            ("kraph_query", "Execute graph queries"),
            ("read", "Generic read access"),
            ("write", "Generic write access"),
        ],
        ServiceId::Elektro => &[
            ("elektro_read", "Read electrophysiology data"),
            ("elektro_write", "Write electrophysiology data"),
            ("elektro_analyze", "Run analysis on recordings"),
            ("read", "Generic read access"),
            ("write", "Generic write access"),
        ],
        ServiceId::Alpaka => &[
            ("alpaka_infer", "Run inference on models"),
            ("alpaka_train", "Train ML models"),
            ("alpaka_manage", "Manage model registry"),
            ("read", "Generic read access"),
            ("write", "Generic write access"),
        ],
        ServiceId::Lovekit => &[],
    }
}

/// Display metadata the manifest carries for each service: name, description, repository.
fn describe(id: ServiceId) -> (&'static str, &'static str, &'static str) {
    match id {
        ServiceId::Rekuest => (
            "Rekuest",
            "Task orchestration and workflow execution",
            "https://github.com/arkitektio/rekuest-server-next",
        ),
        ServiceId::Mikro => (
            "Mikro",
            "Microscopy data management and analysis",
            "https://github.com/arkitektio/mikro-server-next",
        ),
        ServiceId::Fluss => (
            "Fluss",
            "Workflow definition and management",
            "https://github.com/arkitektio/fluss-server-next",
        ),
        ServiceId::Kabinet => (
            "Kabinet",
            "Container and deployment management",
            "https://github.com/arkitektio/kabinet-server",
        ),
        ServiceId::Kraph => (
            "Kraph",
            "Knowledge graph and data relationships",
            "https://github.com/arkitektio/kraph-server",
        ),
        ServiceId::Elektro => (
            "Elektro",
            "Electrophysiology data management",
            "https://github.com/arkitektio/elektro-server",
        ),
        ServiceId::Alpaka => (
            "Alpaka",
            "AI/ML model management",
            "https://github.com/arkitektio/alpaka-server",
        ),
        ServiceId::Lovekit => (
            "Lovekit",
            "LiveKit integration for real-time communication",
            "https://github.com/arkitektio/lovekit-server",
        ),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicSource {
    pub kind: String,
    pub url: String,
}

/// How widely an advertised address is expected to work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AliasScope {
    Local,
    Network,
    Public,
    Ionscale,
}

/// `StagingAlias.scope` defaults to `local` server-side, which is too narrow for anything
/// this machine advertises: a LAN address claiming to be local would never be tried from
/// another machine.
///
/// This is the one place where the ten categories `hosts` distinguishes collapse onto the
/// four values the coordination server accepts. Keep it total, and keep it here: a
/// category that collapses earlier — a tailnet address indistinguishable from a public
/// one, which is what used to happen — is how the wrong scope reaches the wire.
pub fn alias_scope(host: &str, kind: HostCategory) -> AliasScope {
    // The literals first, whatever the category says: a hand-written `localhost` is local
    // even if nobody classified it.
    if matches!(host, "localhost" | "127.0.0.1" | "::1") {
        return AliasScope::Local;
    }
    // A tailnet name under a self-hosted ionscale does not end in `.ts.net`, so the
    // category is the reliable signal and this is only a backstop.
    if host.ends_with(".ts.net") {
        return AliasScope::Ionscale;
    }

    match kind {
        HostCategory::Loopback => AliasScope::Local,
        HostCategory::Mesh => AliasScope::Ionscale,
        // Not this hub's tailnet, so `ionscale` would be a lie — the coordination
        // server's peers are not on it. Only the machines sharing that tailnet can use
        // this, which is closer to a network address than anything else on offer.
        HostCategory::OtherMesh => AliasScope::Network,
        HostCategory::Public | HostCategory::VerifiedFqdn => AliasScope::Public,
        HostCategory::Private
        | HostCategory::MdnsName
        | HostCategory::BareHostname
        | HostCategory::Fqdn => AliasScope::Network,
        // Neither should ever be advertised. If one is, a scope nobody outside this
        // machine will try is the safe floor.
        HostCategory::Virtual | HostCategory::LinkLocal => AliasScope::Local,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagingAlias {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub path: Option<String>,
    pub ssl: bool,
    pub challenge: Option<String>,
    pub kind: String,
    pub scope: AliasScope,
    /// Only true for addresses the coordination server could health check itself.
    pub public: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceManifest {
    pub identifier: String,
    pub version: String,
    pub description: Option<String>,
    pub logo: Option<String>,
    pub roles: Vec<ManifestEntry>,
    pub scopes: Vec<ManifestEntry>,
    pub node_id: Option<String>,
    pub instance_id: String,
    pub public_sources: Vec<PublicSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceRequest {
    pub identifier: String,
    pub description: Option<String>,
    pub manifest: ServiceManifest,
    pub aliases: Vec<StagingAlias>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubManifest {
    pub identifier: String,
    pub description: Option<String>,
    pub logo: Option<String>,
    pub instances: Vec<InstanceRequest>,
    pub clients: Vec<serde_json::Value>,
    /// Ask the coordination server to mint a tailnet pre-auth key for this hub while it
    /// accepts it. Whoever approves decides whether to grant one, so the envelope can come
    /// back without a key even when this is set.
    pub request_auth_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubStartRequest {
    pub hub: HubManifest,
    pub expiration_time_seconds: u64,
}

/// The gateway port clients should reach these services on.
pub fn advertised_port(config: &HubConfig) -> u16 {
    if config.gateway.ssl {
        config.gateway.exposed_https_port.unwrap_or(443)
    } else {
        config.gateway.exposed_http_port.unwrap_or(80)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvertisedHost {
    pub host: String,
    pub kind: HostCategory,
}

/// Turns the chosen hosts into the aliases one service instance advertises.
///
/// `reachable` is the subset an external prober actually reached. It is the only thing
/// that may set `public`, because that flag invites the coordination server to health
/// check the alias itself — and a hub on a LAN address it cannot reach would then look
/// permanently unhealthy.
pub fn build_aliases(
    hosts: &[AdvertisedHost],
    port: u16,
    ssl: bool,
    path: &str,
    reachable: &[String],
) -> Vec<StagingAlias> {
    hosts
        .iter()
        .map(|h| StagingAlias {
            id: h.host.clone(),
            name: h.host.clone(),
            host: h.host.clone(),
            port,
            path: Some(path.to_string()),
            ssl,
            challenge: Some("ht".to_string()),
            kind: "absolute".to_string(),
            scope: alias_scope(&h.host, h.kind),
            // Nothing here is guaranteed to be reachable from the coordination server, so
            // it must not try to health check anything a probe has not confirmed.
            public: reachable.contains(&h.host),
        })
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct HubManifestOptions {
    /// Unique within the organization that accepts the hub.
    pub identifier: String,
    pub description: Option<String>,
    /// Stable per-machine id, so a re-authorized hub is recognised as the same node.
    pub node_id: Option<String>,
    pub hosts: Vec<AdvertisedHost>,
    /// Of `hosts`, the ones an external probe reached. Empty unless somebody checked.
    pub reachable_hosts: Vec<String>,
    pub request_auth_key: bool,
    pub expiration_seconds: Option<u64>,
}

pub fn build_hub_request(config: &HubConfig, options: &HubManifestOptions) -> HubStartRequest {
    let ssl = config.gateway.ssl;
    let port = advertised_port(config);

    let instances = HUB_SERVICE_ORDER
        .into_iter()
        .filter(|id| {
            let block = config.service(*id);
            block.enabled && block.image.is_some()
        })
        .map(|id| {
            let block = config.service(id);
            let (name, description, repo) = describe(id);

            InstanceRequest {
                identifier: name.to_string(),
                description: Some(description.to_string()),
                manifest: ServiceManifest {
                    identifier: format!("live.arkitekt.{}", id.as_str()),
                    version: "1.0.0".to_string(),
                    description: Some(description.to_string()),
                    logo: None,
                    roles: entries(roles_of(id)),
                    scopes: entries(scopes_of(id)),
                    node_id: options.node_id.clone(),
                    instance_id: "default".to_string(),
                    public_sources: vec![PublicSource {
                        kind: "github".to_string(),
                        url: repo.to_string(),
                    }],
                },
                aliases: build_aliases(
                    &options.hosts,
                    port,
                    ssl,
                    &block.host,
                    &options.reachable_hosts,
                ),
            }
        })
        .collect();

    HubStartRequest {
        hub: HubManifest {
            identifier: options.identifier.clone(),
            description: options.description.clone(),
            logo: None,
            instances,
            clients: Vec::new(),
            request_auth_key: options.request_auth_key,
        },
        expiration_time_seconds: options.expiration_seconds.unwrap_or(600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::hub::{build_hub_config, HubConfigOptions};

    #[test]
    fn scopes_an_address_by_how_far_it_reaches() {
        // The literals win over whatever category came with them.
        assert_eq!(alias_scope("localhost", HostCategory::Private), AliasScope::Local);
        assert_eq!(alias_scope("127.0.0.1", HostCategory::Private), AliasScope::Local);
        assert_eq!(
            alias_scope("hub.tail1234.ts.net", HostCategory::Private),
            AliasScope::Ionscale
        );

        assert_eq!(alias_scope("127.0.0.53", HostCategory::Loopback), AliasScope::Local);
        assert_eq!(alias_scope("10.0.0.4", HostCategory::Private), AliasScope::Network);
        assert_eq!(alias_scope("140.78.80.150", HostCategory::Public), AliasScope::Public);

        // The bug this table exists for: a tailnet address is not a public one.
        assert_eq!(
            alias_scope("100.116.108.106", HostCategory::Mesh),
            AliasScope::Ionscale
        );

        assert_eq!(alias_scope("hub.local", HostCategory::MdnsName), AliasScope::Network);
        assert_eq!(alias_scope("hub", HostCategory::BareHostname), AliasScope::Network);
        assert_eq!(alias_scope("hub.example.org", HostCategory::Fqdn), AliasScope::Network);
        assert_eq!(
            alias_scope("hub.example.org", HostCategory::VerifiedFqdn),
            AliasScope::Public
        );

        // Never advertised, but if one slips through it must not be offered to peers.
        assert_eq!(alias_scope("172.17.0.1", HostCategory::Virtual), AliasScope::Local);
        assert_eq!(alias_scope("169.254.1.1", HostCategory::LinkLocal), AliasScope::Local);
    }

    /// The wire vocabulary is fixed at four values, and the server that validates them is
    /// not in this repository. This makes "we cannot accidentally send a fifth" a test
    /// rather than a habit.
    #[test]
    fn every_category_maps_onto_a_scope_the_server_knows() {
        const KNOWN: [&str; 4] = ["local", "network", "public", "ionscale"];

        for scope in [
            AliasScope::Local,
            AliasScope::Network,
            AliasScope::Public,
            AliasScope::Ionscale,
        ] {
            let json = serde_json::to_string(&scope).expect("serializes");
            assert!(KNOWN.contains(&json.trim_matches('"')), "unexpected scope {json}");
        }

        for kind in [
            HostCategory::Loopback,
            HostCategory::Private,
            HostCategory::Mesh,
            HostCategory::Public,
            HostCategory::Virtual,
            HostCategory::LinkLocal,
            HostCategory::MdnsName,
            HostCategory::BareHostname,
            HostCategory::Fqdn,
            HostCategory::VerifiedFqdn,
        ] {
            let scope = alias_scope("hub.example.org", kind);
            let json = serde_json::to_string(&scope).expect("serializes");
            assert!(
                KNOWN.contains(&json.trim_matches('"')),
                "{kind:?} produced {json}"
            );
        }
    }

    /// The gateway is one socket, so an alias is reachable or it is not — the service
    /// path does not enter into it, and a probe of one alias settles them all.
    #[test]
    fn marks_an_alias_public_only_when_a_probe_confirmed_it() {
        let hosts = vec![
            AdvertisedHost {
                host: "140.78.80.150".to_string(),
                kind: HostCategory::Public,
            },
            AdvertisedHost {
                host: "10.0.0.4".to_string(),
                kind: HostCategory::Private,
            },
        ];

        let unconfirmed = build_aliases(&hosts, 80, false, "mikro", &[]);
        assert!(unconfirmed.iter().all(|a| !a.public));

        let confirmed = build_aliases(&hosts, 80, false, "mikro", &["140.78.80.150".to_string()]);
        assert!(confirmed[0].public);
        assert!(!confirmed[1].public);
    }

    /// Lovekit has no image, so advertising it would register an instance nothing serves.
    #[test]
    fn advertises_only_the_services_that_actually_run() {
        let config = build_hub_config(&HubConfigOptions {
            services: Some(vec![ServiceId::Mikro, ServiceId::Lovekit]),
            ..Default::default()
        });
        let request = build_hub_request(&config, &HubManifestOptions::default());

        let ids: Vec<&str> = request
            .hub
            .instances
            .iter()
            .map(|i| i.manifest.identifier.as_str())
            .collect();
        assert!(ids.contains(&"live.arkitekt.rekuest"));
        assert!(ids.contains(&"live.arkitekt.mikro"));
        assert!(!ids.iter().any(|i| i.contains("lovekit")), "{ids:?}");
    }

    #[test]
    fn asks_for_a_mesh_key_only_when_told_to() {
        let config = build_hub_config(&HubConfigOptions::default());

        assert!(!build_hub_request(&config, &HubManifestOptions::default()).hub.request_auth_key);
        assert!(
            build_hub_request(
                &config,
                &HubManifestOptions {
                    request_auth_key: true,
                    ..Default::default()
                }
            )
            .hub
            .request_auth_key
        );
    }
}
