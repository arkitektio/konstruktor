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
    /// Empty only on a [`mesh_alias`], and then left out of the JSON: the coordination
    /// server fills it in from the tailnet node.
    #[serde(default, skip_serializing_if = "String::is_empty")]
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
            challenge: Some(crate::health::HEALTH_PATH.to_string()),
            kind: "absolute".to_string(),
            scope: alias_scope(&h.host, h.kind),
            // Nothing here is guaranteed to be reachable from the coordination server, so
            // it must not try to health check anything a probe has not confirmed.
            public: reachable.contains(&h.host),
        })
        .collect()
}

/// `StagingAlias.kind` for the placeholder the coordination server resolves.
pub const MESH_ALIAS_KIND: &str = "mesh";

/// The alias a hub on a mesh declares for its tailnet node, without knowing its address.
///
/// Nothing on this machine can name it: the tailnet interface lives in the sidecar's
/// network namespace, where no host scan reaches, and the name the node finally gets —
/// deduplicated by ionscale, under a MagicDNS domain only the server knows — is the
/// coordination server's to say. So the hub declares *that* it is on the mesh, with the
/// port and path the gateway serves there, and the server fills in the host from the
/// node that registered with the key it minted — so no host is sent at all.
///
/// The port is the gateway's own, not the host-published one — on the tailnet, Caddy is
/// reached inside the sidecar's namespace, where no port mapping applies.
pub fn mesh_alias(ssl: bool, path: &str) -> StagingAlias {
    StagingAlias {
        id: MESH_ALIAS_KIND.to_string(),
        name: MESH_ALIAS_KIND.to_string(),
        host: String::new(),
        port: if ssl { 443 } else { 80 },
        path: Some(path.to_string()),
        ssl,
        challenge: Some(crate::health::HEALTH_PATH.to_string()),
        kind: MESH_ALIAS_KIND.to_string(),
        scope: AliasScope::Ionscale,
        // The coordination server can health check this itself once it has resolved it;
        // until then there is nothing to check.
        public: false,
    }
}

/// The gateway as the containers on the hub's own docker network reach it — plugin
/// apps an engine or Kabinet starts beside the stack. Same port reasoning as
/// [`mesh_alias`]: inside the network nothing is mapped. `local` is the scope that fits
/// best of the four: nothing outside this machine can use it.
pub fn internal_alias(host: &str, ssl: bool, path: &str) -> StagingAlias {
    StagingAlias {
        id: "internal".to_string(),
        name: "internal".to_string(),
        host: host.to_string(),
        port: if ssl { 443 } else { 80 },
        path: Some(path.to_string()),
        ssl,
        challenge: Some(crate::health::HEALTH_PATH.to_string()),
        kind: "absolute".to_string(),
        scope: AliasScope::Local,
        public: false,
    }
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
    /// The hub is — or is about to be — on a mesh. Adds a [`mesh_alias`] to every
    /// instance.
    pub mesh_alias: bool,
    /// The gateway's name on the hub's docker network, for plugin apps running beside
    /// it. Adds an [`internal_alias`] to every instance. Set for a mesh-only hub, where
    /// it is the only non-tailnet address advertised.
    pub internal_host: Option<String>,
    pub expiration_seconds: Option<u64>,
}

/// The manifest identifier the object store is advertised under, as upstream names it.
pub const S3_MANIFEST: &str = "live.arkitekt.s3";

pub fn build_hub_request(config: &HubConfig, options: &HubManifestOptions) -> HubStartRequest {
    let ssl = config.gateway.ssl;
    let port = advertised_port(config);

    // Every address the hub is reached at, for one path on the gateway: the chosen hosts,
    // the tailnet node, and the gateway's in-network name.
    let aliases_at = |path: &str| {
        let mut aliases = build_aliases(
            &options.hosts,
            port,
            ssl,
            path,
            &options.reachable_hosts,
        );
        if options.mesh_alias {
            aliases.push(mesh_alias(ssl, path));
        }
        if let Some(host) = options.internal_host.as_deref().filter(|h| !h.is_empty()) {
            aliases.push(internal_alias(host, ssl, path));
        }
        aliases
    };

    let mut instances: Vec<InstanceRequest> = HUB_SERVICE_ORDER
        .into_iter()
        .filter(|id| {
            let block = config.service(*id);
            block.enabled && block.image.is_some()
        })
        .map(|id| {
            let block = config.service(id);
            let (name, description, repo) = describe(id);
            let aliases = aliases_at(&block.host);

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
                aliases,
            }
        })
        .collect();

    // The object store, as a datalayer of its own — what upstream's generator registers
    // as `live.arkitekt.s3`. Clients are handed presigned URLs for its buckets and need to
    // know where the store is; the services' own configs only say where *they* reach it,
    // on the stack's private network.
    //
    // No path: S3 clients address buckets path-style from the store's root, and the
    // gateway routes every bucket at its own root — `/mikromedia`, `/kabinetmedia`. The
    // challenge is the store's health check, through the `/rustfs` prefix the gateway
    // strips.
    if config.minio.enabled {
        let aliases = aliases_at("")
            .into_iter()
            .map(|alias| StagingAlias {
                path: None,
                challenge: Some(crate::generate::caddy::S3_CHALLENGE.to_string()),
                ..alias
            })
            .collect();
        instances.push(InstanceRequest {
            identifier: "S3".to_string(),
            description: Some("Object storage for the hub's services".to_string()),
            manifest: ServiceManifest {
                identifier: S3_MANIFEST.to_string(),
                version: "1.0.0".to_string(),
                description: Some("Local S3 / RustFS object storage datalayer".to_string()),
                logo: None,
                roles: Vec::new(),
                scopes: Vec::new(),
                node_id: options.node_id.clone(),
                instance_id: "default".to_string(),
                public_sources: Vec::new(),
            },
            aliases,
        });
    }

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
        assert_eq!(
            alias_scope("localhost", HostCategory::Private),
            AliasScope::Local
        );
        assert_eq!(
            alias_scope("127.0.0.1", HostCategory::Private),
            AliasScope::Local
        );
        assert_eq!(
            alias_scope("hub.tail1234.ts.net", HostCategory::Private),
            AliasScope::Ionscale
        );

        assert_eq!(
            alias_scope("127.0.0.53", HostCategory::Loopback),
            AliasScope::Local
        );
        assert_eq!(
            alias_scope("10.0.0.4", HostCategory::Private),
            AliasScope::Network
        );
        assert_eq!(
            alias_scope("140.78.80.150", HostCategory::Public),
            AliasScope::Public
        );

        // The bug this table exists for: a tailnet address is not a public one.
        assert_eq!(
            alias_scope("100.116.108.106", HostCategory::Mesh),
            AliasScope::Ionscale
        );

        assert_eq!(
            alias_scope("hub.local", HostCategory::MdnsName),
            AliasScope::Network
        );
        assert_eq!(
            alias_scope("hub", HostCategory::BareHostname),
            AliasScope::Network
        );
        assert_eq!(
            alias_scope("hub.example.org", HostCategory::Fqdn),
            AliasScope::Network
        );
        assert_eq!(
            alias_scope("hub.example.org", HostCategory::VerifiedFqdn),
            AliasScope::Public
        );

        // Never advertised, but if one slips through it must not be offered to peers.
        assert_eq!(
            alias_scope("172.17.0.1", HostCategory::Virtual),
            AliasScope::Local
        );
        assert_eq!(
            alias_scope("169.254.1.1", HostCategory::LinkLocal),
            AliasScope::Local
        );
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
            assert!(
                KNOWN.contains(&json.trim_matches('"')),
                "unexpected scope {json}"
            );
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

        assert!(
            !build_hub_request(&config, &HubManifestOptions::default())
                .hub
                .request_auth_key
        );
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

    /// A hub on a mesh cannot name its own tailnet address, so it declares a placeholder
    /// per instance for the coordination server to resolve — and a hub off the mesh
    /// sends exactly what it always did.
    #[test]
    fn declares_a_mesh_alias_only_for_a_hub_on_a_mesh() {
        let config = build_hub_config(&HubConfigOptions::default());
        let hosts = vec![AdvertisedHost {
            host: "10.0.0.4".to_string(),
            kind: HostCategory::Private,
        }];

        let off = build_hub_request(
            &config,
            &HubManifestOptions {
                hosts: hosts.clone(),
                ..Default::default()
            },
        );
        assert!(off
            .hub
            .instances
            .iter()
            .all(|i| i.aliases.iter().all(|a| a.kind != MESH_ALIAS_KIND)));

        let on = build_hub_request(
            &config,
            &HubManifestOptions {
                hosts,
                mesh_alias: true,
                ..Default::default()
            },
        );
        for instance in &on.hub.instances {
            let mesh: Vec<_> = instance
                .aliases
                .iter()
                .filter(|a| a.kind == MESH_ALIAS_KIND)
                .collect();
            assert_eq!(mesh.len(), 1, "{}", instance.identifier);
            let alias = mesh[0];
            // The server fills the host in; on the wire the key is absent, not empty.
            let json = serde_json::to_value(alias).expect("serializes");
            assert!(json.get("host").is_none(), "{json}");
            assert_eq!(json["kind"], "mesh");
            assert_eq!(alias.scope, AliasScope::Ionscale);
            assert_eq!(alias.port, if config.gateway.ssl { 443 } else { 80 });
            assert!(!alias.public);
            // The LAN alias is still there beside it.
            assert!(instance.aliases.iter().any(|a| a.host == "10.0.0.4"));
        }
    }

    /// The object store is advertised as a datalayer of its own, as upstream does: every
    /// address the services have, at the gateway's root, challenged on the store's health.
    #[test]
    fn advertises_the_object_store_as_an_s3_datalayer() {
        let config = build_hub_config(&HubConfigOptions::default());
        let request = build_hub_request(
            &config,
            &HubManifestOptions {
                hosts: vec![AdvertisedHost {
                    host: "10.0.0.4".to_string(),
                    kind: HostCategory::Private,
                }],
                mesh_alias: true,
                ..Default::default()
            },
        );

        let s3 = request
            .hub
            .instances
            .iter()
            .find(|i| i.manifest.identifier == S3_MANIFEST)
            .expect("an S3 instance");
        let hosts: Vec<(&str, &str)> = s3
            .aliases
            .iter()
            .map(|a| (a.kind.as_str(), a.host.as_str()))
            .collect();
        assert_eq!(hosts, vec![("absolute", "10.0.0.4"), (MESH_ALIAS_KIND, "")]);
        for alias in &s3.aliases {
            // Buckets are routed at the gateway's root; a path would put every presigned
            // URL under a prefix nothing serves.
            let json = serde_json::to_value(alias).unwrap();
            assert_eq!(json["path"], serde_json::Value::Null, "{json}");
            assert_eq!(alias.challenge.as_deref(), Some("rustfs/health"));
        }
    }

    /// Mesh-only: the tailnet node and the gateway's in-network name, nothing else.
    #[test]
    fn a_mesh_only_hub_advertises_the_mesh_and_the_internal_gateway() {
        let config = build_hub_config(&HubConfigOptions::default());
        let request = build_hub_request(
            &config,
            &HubManifestOptions {
                mesh_alias: true,
                internal_host: Some("gateway".to_string()),
                ..Default::default()
            },
        );

        for instance in &request.hub.instances {
            let shape: Vec<(&str, &str, AliasScope)> = instance
                .aliases
                .iter()
                .map(|a| (a.kind.as_str(), a.host.as_str(), a.scope))
                .collect();
            assert_eq!(
                shape,
                vec![
                    (MESH_ALIAS_KIND, "", AliasScope::Ionscale),
                    ("absolute", "gateway", AliasScope::Local),
                ],
                "{}",
                instance.identifier
            );
        }
    }

    /// Every hub declares its in-network name besides its other addresses, so plugins an
    /// attached engine starts on the hub's network are told the one address they can reach
    /// — on every instance, the object store included.
    #[test]
    fn every_instance_carries_the_internal_gateway_alongside_the_rest() {
        let config = build_hub_config(&HubConfigOptions::default());
        let request = build_hub_request(
            &config,
            &HubManifestOptions {
                hosts: vec![AdvertisedHost {
                    host: "10.0.0.4".to_string(),
                    kind: HostCategory::Private,
                }],
                internal_host: Some("gateway".to_string()),
                ..Default::default()
            },
        );

        assert!(request
            .hub
            .instances
            .iter()
            .any(|i| i.manifest.identifier == S3_MANIFEST));
        for instance in &request.hub.instances {
            assert!(
                instance.aliases.iter().any(|a| a.host == "10.0.0.4"),
                "{}",
                instance.identifier
            );
            let internal = instance
                .aliases
                .iter()
                .find(|a| a.host == "gateway")
                .unwrap_or_else(|| panic!("{} has no internal alias", instance.identifier));
            assert_eq!(internal.scope, AliasScope::Local);
        }
    }
}
