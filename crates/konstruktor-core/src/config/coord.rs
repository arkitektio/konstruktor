//! A coordination server's profile: a [`HubConfig`] with no services, running Lok in public —
//! its issuer an address, not a name — beside the account frontend and, optionally, the mesh.
//!
//! It is a hub profile rather than a file of its own for what it is made of and how it is
//! kept: Postgres, Redis, the object store and the gateway are the hub's, generated the same
//! way, pinned in the same lock, backed up by the same `pg_dumpall` and moved by the same
//! `update`. What makes it a coordination server is [`HubConfig::coord`], which only it has.
//!
//! What it does not share with a self-contained hub's Lok is the trust: there Lok is a root
//! of trust nobody outside the machine relies on, advertising itself at whatever address it
//! was asked at. Here every hub, app and browser that reaches it relies on its tokens naming
//! one address, so its issuer is that address, and it only answers there.

use serde::{Deserialize, Serialize};

use crate::config::hub::{
    build_hub_config, database_name, HubConfig, HubConfigOptions, LokOptions, LokOrganization,
    LokUser, PublicSite, StorageMode, LOCAL_COORD_SERVER, MAIN_DATABASE,
};
use crate::connect::manifest::AdvertisedHost;
use crate::hosts::HostCategory;
use crate::secrets::{generate_alpha_numeric_string, KeyPair};

/// Lok on a coordination server follows its major: the one this build writes the config of.
pub const COORD_LOK_IMAGE: &str = "jhnnsrs/lok:8";

/// The account frontend, which only works served at the root of Lok's own origin: it reaches
/// Lok at `/lok`, on the session it holds there. Its API is Lok's, so it moves with Lok.
pub const KONTROL_IMAGE: &str = "jhnnsrs/kontrol:4";

/// The mesh control server, until Lok's image says which one it runs beside it (its
/// `ionskale` sidecar, which is what is used when it does).
pub const IONSKALE_IMAGE: &str = "jhnnsrs/ionskale:latest";

/// What the stack's clients of a CA nobody else trusts — ionskale fetching Lok's OIDC
/// configuration — are given as their roots, in the folder.
pub const TRUSTED_ROOTS_FILE: &str = "configs/trust/roots.pem";

/// What a coordination server runs beside Lok, and what Lok is told of its place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordBlock {
    /// What it calls itself: the deployment's name in the well-known, on the landing page.
    pub name: String,
    /// Where it is reached, as a browser spells the origin: `https://coord.example.org`, or
    /// `http://localhost:8080` for one that serves plain HTTP in a lab. Lok's issuer: every
    /// token it mints names it, and every service that takes them matches it exactly.
    pub origin: String,
    pub kontrol: KontrolBlock,
    /// The mesh: one tailnet per organization, kept in step with its members by Lok.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<IonskaleBlock>,
    /// Where Lok sends mail from. Without it there are no login codes and no verification
    /// mails — accounts are made by an administrator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<SmtpBlock>,
    pub signup: SignupPolicy,
    /// Roots, PEM, that the stack's own clients need beside the web PKI's: what
    /// `KONSTRUKTOR_CA_FILE` named when it was created. See [`TRUSTED_ROOTS_FILE`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_roots: Option<String>,
}

impl CoordBlock {
    /// The host of [`Self::origin`]: what Lok answers to.
    pub fn host(&self) -> &str {
        host_of(&self.origin)
    }

    /// Whether it is reached over TLS.
    pub fn https(&self) -> bool {
        self.origin.starts_with("https://")
    }
}

/// Who may create an account.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignupPolicy {
    /// Accounts are made by an administrator, or by invitation. What a server on the
    /// internet starts out as.
    #[default]
    Closed,
    /// Anybody who reaches it can sign up.
    Open,
}

impl SignupPolicy {
    pub fn is_open(self) -> bool {
        self == SignupPolicy::Open
    }
}

/// The account frontend's container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KontrolBlock {
    pub host: String,
    pub image: String,
    pub internal_port: u16,
}

/// The mesh control server (ionskale), served at a name of its own on the same gateway:
/// Tailscale clients reach control and relay at the root of a name, so it cannot share Lok's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IonskaleBlock {
    pub host: String,
    pub image: String,
    pub internal_port: u16,
    /// Where nodes reach it: `mesh.coord.example.org`, on 443.
    pub domain: String,
    /// The UDP port its STUN server answers on, published as it is: clients are told it.
    pub stun_port: u16,
    /// What MagicDNS names end in.
    pub dns_suffix: String,
    /// Its database, in the stack's Postgres.
    pub db: String,
    /// The bearer token Lok drives it with (`svc_…`).
    pub service_token: String,
    /// Lok's OpenID client it logs its users in as.
    pub client_id: String,
    pub client_secret: String,
}

impl IonskaleBlock {
    /// `https://<domain>`: what nodes are given as their control server.
    pub fn url(&self) -> String {
        format!("https://{}", self.domain)
    }

    /// Where Lok sends its users back to after they logged in to the mesh.
    pub fn callback_url(&self) -> String {
        format!("{}/a/callback", self.url())
    }
}

/// The SMTP server Lok sends mail through.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmtpBlock {
    pub host: String,
    pub port: u16,
    /// STARTTLS.
    pub use_tls: bool,
    pub user: String,
    pub password: String,
    /// The `From` of what it sends.
    pub from: String,
}

/// How a coordination server's mesh is asked for.
#[derive(Debug, Clone)]
pub struct MeshChoice {
    pub domain: String,
    /// Left out, `ts.<domain of the coordination server>`.
    pub dns_suffix: Option<String>,
}

/// What a front end says about a coordination server.
#[derive(Debug, Clone)]
pub struct CoordOptions {
    pub device_id: String,
    pub name: String,
    /// The origin it is reached at. See [`CoordBlock::origin`].
    pub origin: String,
    /// The name it is served at with a publicly valid certificate; `None` for one that
    /// serves plain HTTP.
    pub public: Option<PublicSite>,
    pub http_port: Option<u16>,
    pub https_port: Option<u16>,
    pub admin: String,
    pub admin_password: Option<String>,
    pub admin_email: Option<String>,
    /// The organization the administrator starts out in, which hubs can be accepted into.
    pub organization: String,
    pub mesh: Option<MeshChoice>,
    pub email: Option<SmtpBlock>,
    pub signup: SignupPolicy,
    pub storage: StorageMode,
    pub extra_roots: Option<String>,
    /// Injected by the tests; generated fresh otherwise.
    pub key_pair: Option<KeyPair>,
}

/// The id of a coordination server's first organization.
const FIRST_ORGANIZATION: i64 = 1;

/// Builds a coordination server's profile.
///
/// A hub profile of no services, whose Lok issues tokens as [`CoordOptions::origin`]: a
/// self-contained hub's Lok is the starting point, and what makes it public is undone here
/// rather than threaded through every builder of hubs — its mandate keys and its
/// preconfigured hub go, its one account becomes the administrator.
pub fn build_coord_config(options: &CoordOptions) -> HubConfig {
    let ssl = options.public.is_some();
    let host = host_of(&options.origin).to_string();
    let mut config = build_hub_config(&HubConfigOptions {
        device_id: options.device_id.clone(),
        coord_server: LOCAL_COORD_SERVER.into(),
        // Nobody's Rekuest: a coordination server runs none of a hub's services.
        rekuest_server: String::new(),
        services: Some(Vec::new()),
        http_port: options.http_port,
        https_port: options.https_port,
        ssl,
        domain: Some(host.clone()),
        global_admin: options.admin.clone(),
        global_admin_password: options.admin_password.clone(),
        global_admin_email: options.admin_email.clone(),
        global_description: None,
        csrf_trusted_origins: Some(vec![options.origin.clone()]),
        lok: Some(LokOptions {
            hub_identifier: options.name.clone(),
            hosts: vec![AdvertisedHost {
                host: host.clone(),
                kind: if ssl {
                    HostCategory::VerifiedFqdn
                } else {
                    crate::hosts::classify_host(&host)
                },
            }],
            organization: options.organization.clone(),
            user: options.admin.clone(),
            user_password: None,
            mandate_keys: Vec::new(),
            key_pair: options.key_pair.clone(),
        }),
        storage: options.storage,
        ..Default::default()
    });
    config.gateway.public = options.public.clone();

    let lok = config.lok.as_mut().expect("a coordination server runs Lok");
    lok.image = COORD_LOK_IMAGE.into();
    lok.issuer = options.origin.clone();
    lok.mandate_keys.clear();
    lok.organization = LokOrganization {
        id: FIRST_ORGANIZATION,
        identifier: slug(&options.organization),
        name: options.organization.clone(),
    };
    lok.users = vec![LokUser {
        username: config.global_admin.clone(),
        password: config.global_admin_password.clone(),
        email: config.global_admin_email.clone(),
        superuser: true,
    }];

    let suffix_base = host.clone();
    config.coord = Some(CoordBlock {
        name: options.name.clone(),
        origin: options.origin.clone(),
        kontrol: KontrolBlock {
            host: "kontrol".into(),
            image: KONTROL_IMAGE.into(),
            internal_port: 80,
        },
        mesh: options.mesh.as_ref().map(|mesh| IonskaleBlock {
            host: "ionskale".into(),
            image: IONSKALE_IMAGE.into(),
            internal_port: 8080,
            domain: mesh.domain.clone(),
            stun_port: 3478,
            dns_suffix: mesh
                .dns_suffix
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| format!("ts.{suffix_base}")),
            db: database_name("ionskale", MAIN_DATABASE).expect("a plain name"),
            service_token: format!("svc_{}", generate_alpha_numeric_string(48)),
            client_id: "ionskale".into(),
            client_secret: generate_alpha_numeric_string(48),
        }),
        email: options.email.clone(),
        signup: options.signup,
        extra_roots: options.extra_roots.clone(),
    });
    config
}

/// The host of an origin: `https://coord.example.org:8443` → `coord.example.org`.
pub fn host_of(origin: &str) -> &str {
    let rest = origin.split_once("://").map_or(origin, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or(rest);
    if let Some(bracketed) = authority.strip_prefix('[') {
        return bracketed.split(']').next().unwrap_or(bracketed);
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => authority,
    }
}

/// Lok's handle for an organization: lowercase letters, digits and dashes.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "main".into()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::hub::PublicTls;

    pub(crate) fn options() -> CoordOptions {
        CoordOptions {
            device_id: "device".into(),
            name: "Lab coordination".into(),
            origin: "https://coord.example.org".into(),
            public: Some(PublicSite {
                domain: "coord.example.org".into(),
                extra_names: vec!["mesh.coord.example.org".into()],
                tls: PublicTls::Acme {
                    email: "ops@example.org".into(),
                    ca: None,
                    ca_root: None,
                },
            }),
            http_port: Some(80),
            https_port: Some(443),
            admin: "admin".into(),
            admin_password: Some("a-password".into()),
            admin_email: Some("ops@example.org".into()),
            organization: "Example Lab".into(),
            mesh: Some(MeshChoice {
                domain: "mesh.coord.example.org".into(),
                dns_suffix: None,
            }),
            email: None,
            signup: SignupPolicy::Closed,
            storage: StorageMode::default(),
            extra_roots: None,
            key_pair: None,
        }
    }

    #[test]
    fn a_coordination_server_runs_lok_as_its_origin_and_no_service() {
        let config = build_coord_config(&options());
        assert!(config.is_coord());
        assert!(config.enabled_services().is_empty());
        let lok = config.running_lok().unwrap();
        assert_eq!(lok.issuer, "https://coord.example.org");
        assert_eq!(lok.image, COORD_LOK_IMAGE);
        assert!(lok.mandate_keys.is_empty());
        assert_eq!(lok.users.len(), 1);
        assert!(lok.users[0].superuser);
        assert_eq!(lok.users[0].username, config.global_admin);
        assert_eq!(lok.users[0].password, "a-password");
        assert_eq!(lok.organization.identifier, "example-lab");
        assert!(config.gateway.ssl);
        assert_eq!(
            config.public_origin().as_deref(),
            Some("https://coord.example.org")
        );
    }

    #[test]
    fn its_mesh_gets_a_suffix_of_its_own_and_fresh_secrets() {
        let config = build_coord_config(&options());
        let mesh = config.coord.as_ref().unwrap().mesh.as_ref().unwrap();
        assert_eq!(mesh.dns_suffix, "ts.coord.example.org");
        assert!(mesh.service_token.starts_with("svc_"));
        // ionskale wants at least 32 characters after the prefix.
        assert!(mesh.service_token.len() >= 4 + 32);
        assert_eq!(
            mesh.callback_url(),
            "https://mesh.coord.example.org/a/callback"
        );
        assert_eq!(mesh.db, "ionskale_main");
    }

    #[test]
    fn a_lab_one_serves_plain_http_at_its_address() {
        let mut lab = options();
        lab.origin = "http://localhost:8080".into();
        lab.public = None;
        lab.mesh = None;
        let config = build_coord_config(&lab);
        assert!(!config.gateway.ssl);
        assert_eq!(
            config.running_lok().unwrap().issuer,
            "http://localhost:8080"
        );
        let coord = config.coord.as_ref().unwrap();
        assert_eq!(coord.host(), "localhost");
        assert!(!coord.https());
    }

    #[test]
    fn hosts_and_slugs() {
        assert_eq!(host_of("https://coord.example.org"), "coord.example.org");
        assert_eq!(host_of("http://localhost:8080"), "localhost");
        assert_eq!(host_of("http://[::1]:8080/x"), "::1");
        assert_eq!(slug("Example Lab!"), "example-lab");
        assert_eq!(slug("  "), "main");
    }

    #[test]
    fn a_hub_profile_is_not_a_coordination_server() {
        let hub = build_hub_config(&HubConfigOptions::default());
        assert!(!hub.is_coord());
        let yaml = serde_norway::to_string(&hub).unwrap();
        assert!(!yaml.contains("coord:"), "a hub writes no coord block");
    }
}
