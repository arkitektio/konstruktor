pub mod caddy;
pub mod compose;
pub mod coord;
pub mod lok;
pub mod service;
pub mod write;

use std::collections::BTreeMap;

use serde_norway::Value;

use crate::config::hub::HubConfig;

/// The deployment generator — a port of the hub path through
/// `arkitekt_next/server/diff.py` (`write_hub_files` and the helpers it shares).
///
/// Everything here is a pure function over the profile; writing to disk is [`write`].

/// Relative POSIX paths inside the deployment folder → file contents.
pub type GeneratedFiles = BTreeMap<String, String>;

/// What the coordination server told us about itself when it authorized this hub.
#[derive(Debug, Clone, Default)]
pub struct IssuedIdentity {
    /// The `iss` claim inbound tokens carry, from the well-known. Authentikate selects a
    /// trust anchor by strict string equality, so this is not a label: with the wrong
    /// value every token from the coordination server is rejected as untrusted.
    pub issuer: Option<String>,
    /// Where that issuer's verification keys live, from the grant envelope.
    pub jwks_url: Option<String>,
    /// The organizations the hub was accepted into (each grant's `org`): the only ones its
    /// services take tokens of. A hub names itself, and anyone can claim its name at the
    /// coordination server in an organization of their own; what the hub serves is decided
    /// here, by the grants it holds. Empty when no grant says — then nothing is narrowed.
    pub organizations: Vec<String>,
}

/// Python's `yaml.dump(..., default_flow_style=False)`: block style, sorted keys.
///
/// Sorting comes from serializing through `BTreeMap`/`Value`, which orders mappings by
/// key. The output is not byte-identical to PyYAML's — nor was the TypeScript's, which
/// diverged on sequence indentation, quote style and block scalars — and nothing requires
/// it to be. What it must be is *semantically* identical, which the golden tests check by
/// parsing both sides.
pub(crate) fn dump(value: &Value) -> String {
    serde_norway::to_string(value).expect("a generated document always serializes")
}

/// Every file a hub deployment consists of, keyed by its path in the folder.
pub fn generate_hub_files(
    config: &HubConfig,
    _issued: &IssuedIdentity,
    said: &crate::contract::Said,
) -> GeneratedFiles {
    let enabled = config.enabled_services();
    let mut files = GeneratedFiles::new();

    // A service's own config is not among these: its image writes that, from the hub's
    // facts ([`crate::contract`]).

    // --- the coordination server, on a hub that runs its own ------------------
    let lok = config.running_lok();
    if let Some(lok) = lok {
        files.insert(
            format!("configs/{}.yaml", lok.host),
            dump(&lok::build_lok_config(config, lok, said)),
        );
        let access = match config.running_coord() {
            Some(coord) => coord::build_access(config, lok, coord),
            None => lok::build_access(config, lok),
        };
        let mut access =
            serde_json::to_string_pretty(&access).expect("the access document is plain data");
        access.push('\n');
        files.insert(lok::ACCESS_FILE.to_string(), access);
    }

    // --- what a coordination server runs beside Lok ---------------------------
    if let Some(coord) = config.running_coord() {
        let mut landing = serde_json::to_string_pretty(&coord::build_landing(coord))
            .expect("the landing page is plain data");
        landing.push('\n');
        files.insert(coord::LANDING_FILE.to_string(), landing);
        if let Some(mesh) = &coord.mesh {
            files.insert(
                coord::IONSKALE_CONFIG_FILE.to_string(),
                dump(&coord::build_ionskale_config(config, coord, mesh)),
            );
        }
        if let Some(roots) = &coord.extra_roots {
            files.insert(
                crate::config::coord::TRUSTED_ROOTS_FILE.to_string(),
                roots.clone(),
            );
        }
    }

    // --- secret files, mounted read-only into the one service that reads each ---
    for id in &enabled {
        let block = config.service(*id);
        for (name, secret) in &block.secrets {
            files.insert(service::secret_file(block, name), format!("{secret}\n"));
        }
    }

    // --- the plugin engine's mandate key, which the compose file only names ----
    if let Some(engine) = config.running_plugin_engine() {
        files.insert(
            compose::PLUGIN_ENGINE_ENV_FILE.to_string(),
            format!("FAKTS_MANDATE_KEY={}\n", engine.mandate_key),
        );
    }

    // --- Lovekit's media server ------------------------------------------------
    if let Some(livekit) = config.running_livekit() {
        files.insert(
            format!("configs/{}.yaml", livekit.host),
            dump(&compose::build_livekit_config(livekit)),
        );
    }

    // --- the mesh sidecar's key, which the compose file only names -------------
    if let Some(mesh) = config.mesh.as_ref().filter(|m| m.enabled) {
        files.insert(
            crate::config::mesh::MESH_ENV_FILE.to_string(),
            mesh.env_file_contents(),
        );
    }

    // --- minio's bucket manifest --------------------------------------------
    if let Some(minio_init) = compose::build_minio_init(config, &enabled) {
        files.insert(
            format!("configs/{}.yaml", config.minio.init_container_host),
            dump(&minio_init),
        );
    }

    // --- gateway ------------------------------------------------------------
    let caddy_services: Vec<caddy::CaddyService<'_>> = enabled
        .iter()
        .map(|&id| {
            let block = config.service(id);
            caddy::CaddyService {
                id,
                host: &block.host,
                internal_port: block.internal_port,
                buckets: block.buckets.names(),
                agent_upstream: (id == crate::catalog::ServiceId::Rekuest)
                    .then(|| config.takt_host())
                    .flatten()
                    .map(|host| caddy::AgentUpstream {
                        host,
                        port: crate::config::hub::TAKT_INTERNAL_PORT,
                    }),
            }
        })
        .collect();

    let caddyfile = match (config.running_coord(), lok) {
        (Some(coord), Some(lok)) => caddy::build_coord_caddyfile(&coord_sites(config, coord, lok)),
        _ => caddy::build_caddyfile(
            &caddy_services,
            lok.map(|lok| caddy::Upstream {
                name: &lok.host,
                target: &lok.host,
                port: lok.internal_port,
                buckets: vec![lok.media_bucket.bucket_name.clone()],
            })
            .as_ref(),
            &config.minio.host,
            config.minio.internal_port,
            &caddy::GatewaySites {
                livekit: config.running_livekit().map(|livekit| caddy::LivekitSite {
                    listen_port: livekit.signal_port,
                    tls_listen_port: config.served_certificate().map(|_| livekit.signal_tls_port),
                    upstream_host: &livekit.host,
                    upstream_port: crate::config::hub::LIVEKIT_INTERNAL_PORT,
                }),
                tls: config.served_certificate().map(|_| caddy::GatewayTls {
                    cert_file: format!("{}/{}", crate::tls::CONTAINER_DIR, crate::tls::CERT_FILE),
                    key_file: format!("{}/{}", crate::tls::CONTAINER_DIR, crate::tls::KEY_FILE),
                }),
            },
        ),
    };
    files.insert("configs/Caddyfile".to_string(), caddyfile);

    // --- the gateway's certificate, when it serves HTTPS ---------------------
    if let Some(certificate) = config.served_certificate() {
        files.insert(
            format!("{}/{}", crate::tls::FOLDER_DIR, crate::tls::CERT_FILE),
            certificate.cert_pem.clone(),
        );
        files.insert(
            format!("{}/{}", crate::tls::FOLDER_DIR, crate::tls::KEY_FILE),
            certificate.key_pem.clone(),
        );
    }

    // --- the compose project ------------------------------------------------
    files.insert(
        "docker-compose.yaml".to_string(),
        dump(&compose::build_compose(config, &enabled, said)),
    );

    files
}

/// What a coordination server's gateway serves, from its profile.
fn coord_sites<'a>(
    config: &'a HubConfig,
    coord: &'a crate::config::coord::CoordBlock,
    lok: &'a crate::config::hub::LokBlock,
) -> caddy::CoordSites<'a> {
    use crate::config::hub::PublicTls;
    let in_container = |file: &str| format!("{}/{file}", crate::tls::CONTAINER_DIR);
    let site = config.public_site();
    let certificate = site.map(|site| match &site.tls {
        PublicTls::Acme { .. } => caddy::SiteCertificate::Acme,
        PublicTls::Provided { .. } => caddy::SiteCertificate::Files {
            cert_file: in_container(crate::tls::PUBLIC_CERT_FILE),
            key_file: in_container(crate::tls::PUBLIC_KEY_FILE),
        },
    });
    // The name alone: inside its container the gateway listens on 443 whatever port the
    // host publishes that on, and a site matches a request by its host, not its port.
    let address = |name: &str| name.to_string();
    caddy::CoordSites {
        lok: caddy::Upstream {
            name: &lok.host,
            target: &lok.host,
            port: lok.internal_port,
            buckets: vec![lok.media_bucket.bucket_name.clone()],
        },
        kontrol: (&coord.kontrol.host, coord.kontrol.internal_port),
        public: site
            .zip(certificate.clone())
            .map(|(site, certificate)| caddy::PublicSiteBlock {
                address: address(&site.domain),
                certificate,
            }),
        mesh: coord
            .mesh
            .as_ref()
            .zip(certificate)
            .filter(|_| site.is_some())
            .map(|(mesh, certificate)| {
                (
                    caddy::PublicSiteBlock {
                        address: address(&mesh.domain),
                        certificate,
                    },
                    mesh.host.as_str(),
                    mesh.internal_port,
                )
            }),
        acme: site.and_then(|site| match &site.tls {
            PublicTls::Acme { email, ca, ca_root } => Some(caddy::Acme {
                email: email.clone(),
                ca: ca.clone(),
                ca_root: ca_root
                    .as_ref()
                    .map(|_| in_container(crate::tls::ACME_ROOT_FILE)),
            }),
            PublicTls::Provided { .. } => None,
        }),
        public_port: crate::config::hub::public_port(config),
        fallback: config.served_certificate().map(|_| caddy::GatewayTls {
            cert_file: in_container(crate::tls::CERT_FILE),
            key_file: in_container(crate::tls::KEY_FILE),
        }),
        minio_host: &config.minio.host,
        minio_port: config.minio.internal_port,
    }
}

#[cfg(test)]
mod tls_tests {
    use crate::config::hub::{build_hub_config, HubConfigOptions};

    /// A hub serving HTTPS writes its certificate and key, mounts them into the gateway and
    /// serves HTTPS beside plain HTTP, unredirected; a plain-HTTP hub writes none of it.
    #[test]
    fn a_hub_with_a_certificate_writes_and_serves_it() {
        let mut config = build_hub_config(&HubConfigOptions {
            ssl: true,
            ..Default::default()
        });
        config
            .place_certificate(&["lab.local".to_string()])
            .expect("a certificate");
        let files = super::generate_hub_files(&config, &Default::default(), &Default::default());

        let certificate = config.gateway.certificate.as_ref().expect("placed");
        assert_eq!(
            files.get("configs/gateway/hub.crt"),
            Some(&certificate.cert_pem)
        );
        assert_eq!(
            files.get("configs/gateway/hub.key"),
            Some(&certificate.key_pem)
        );
        let caddyfile = files.get("configs/Caddyfile").expect("a Caddyfile");
        assert!(
            caddyfile.contains("auto_https disable_redirects"),
            "{caddyfile}"
        );
        assert!(
            caddyfile.contains("\nhttp:// {\n\timport hub\n}\n"),
            "{caddyfile}"
        );
        assert!(caddyfile.contains("\nhttps:// {\n\ttls /etc/caddy/tls/hub.crt /etc/caddy/tls/hub.key\n\timport hub\n}\n"), "{caddyfile}");
        let compose = files.get("docker-compose.yaml").expect("a compose file");
        assert!(
            compose.contains("./configs/gateway:/etc/caddy/tls:ro"),
            "{compose}"
        );

        let plain = build_hub_config(&HubConfigOptions::default());
        let files = super::generate_hub_files(&plain, &Default::default(), &Default::default());
        assert!(!files.contains_key("configs/gateway/hub.crt"));
        assert!(!files["configs/Caddyfile"].contains("https://"));
        assert!(!files["docker-compose.yaml"].contains("/etc/caddy/tls"));
    }
}
