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
                buckets: id
                    .bucket_purposes()
                    .iter()
                    .filter_map(|purpose| block.bucket(purpose).map(|b| b.bucket_name.clone()))
                    .collect(),
            }
        })
        .collect();

    files.insert(
        "configs/Caddyfile".to_string(),
        caddy::build_caddyfile(
            &caddy_services,
            &config.minio.host,
            config.minio.internal_port,
        ),
    );

    // --- the compose project ------------------------------------------------
    files.insert(
        "docker-compose.yaml".to_string(),
        dump(&compose::build_compose(config, &enabled)),
    );

    files
}
