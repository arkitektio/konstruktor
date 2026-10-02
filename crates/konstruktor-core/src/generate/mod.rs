pub mod caddy;
pub mod compose;
pub mod service;
pub mod write;

use std::collections::BTreeMap;

use serde_norway::Value;

use crate::catalog::HUB_SERVICE_ORDER;
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
    /// The hub's trust bundle (its service instances' public keys), from the grant envelope.
    pub hub_keys_url: Option<String>,
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
pub fn generate_hub_files(config: &HubConfig, issued: &IssuedIdentity) -> GeneratedFiles {
    let enabled = config.enabled_services();
    let mut files = GeneratedFiles::new();

    // --- the services' own configs ------------------------------------------
    for id in &enabled {
        files.insert(
            format!("configs/{}.yaml", config.service(*id).host),
            dump(&service::build_service_config(config, *id, issued)),
        );
    }

    // --- secret files, mounted read-only into the one service that reads each ---
    for id in &enabled {
        let block = config.service(*id);
        if let Some(key) = &block.fernet_key {
            files.insert(service::fernet_key_file(block), format!("{key}\n"));
        }
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
    let caddy_services: Vec<caddy::CaddyService<'_>> = HUB_SERVICE_ORDER
        .into_iter()
        .filter(|id| enabled.contains(id))
        .map(|id| {
            let block = config.service(id);
            caddy::CaddyService {
                id,
                host: &block.host,
                internal_port: block.internal_port,
                buckets: block.bucket_names(id).into_iter().map(|(_, name)| name).collect(),
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

    files.insert(
        "configs/Caddyfile".to_string(),
        caddy::build_caddyfile(
            &caddy_services,
            &config.minio.host,
            config.minio.internal_port,
            &caddy::GatewaySites {
                livekit: config.running_livekit().map(|livekit| caddy::LivekitSite {
                    listen_port: livekit.signal_port,
                    upstream_host: &livekit.host,
                    upstream_port: crate::config::hub::LIVEKIT_INTERNAL_PORT,
                }),
            },
        ),
    );

    // --- the compose project ------------------------------------------------
    files.insert(
        "docker-compose.yaml".to_string(),
        dump(&compose::build_compose(config, &enabled)),
    );

    files
}
