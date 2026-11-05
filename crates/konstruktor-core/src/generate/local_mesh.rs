//! What a self-contained hub's own mesh is generated into: ionskale's config and certificate,
//! its compose service, and what Lok is told of it. See [`crate::config::local_mesh`].

use serde_norway::Value;

use crate::config::local_mesh::{
    LocalMeshBlock, CA_FILE, CERT_FILE, CONFIG_FILE, CONTAINER_TLS_DIR, DATA_DIR, DATA_VOLUME,
    HUB_KEY_FILE, KEY_FILE, LISTEN_PORT, STUN_LISTEN_PORT,
};
use crate::generate::service::{list, map, s};

/// The files, by their path in the folder.
pub fn files(mesh: &LocalMeshBlock) -> Vec<(String, String)> {
    vec![
        (
            CONFIG_FILE.to_string(),
            super::dump(&build_ionskale_config(mesh)),
        ),
        (CA_FILE.to_string(), mesh.ca_pem.clone()),
        (
            CERT_FILE.to_string(),
            // The chain, so a client that was given the CA alone can build it.
            format!("{}{}", mesh.cert_pem, mesh.ca_pem),
        ),
        (KEY_FILE.to_string(), mesh.key_pem.clone()),
    ]
}

fn in_container(file: &str) -> String {
    let name = file.rsplit('/').next().unwrap_or(file);
    format!("{CONTAINER_TLS_DIR}/{name}")
}

/// `configs/ionskale/config.yaml`.
///
/// No OpenID provider: nobody logs in to this mesh, every node joins with a key Lok minted.
/// Lok drives it with the service token; the relay is the embedded one, at the same address.
pub fn build_ionskale_config(mesh: &LocalMeshBlock) -> Value {
    let public = |port: u16| {
        let host = if mesh.address.contains(':') {
            format!("[{}]", mesh.address)
        } else {
            mesh.address.clone()
        };
        format!("{host}:{port}")
    };
    map(vec![
        ("listen_addr", s(&format!(":{LISTEN_PORT}"))),
        ("public_addr", s(&public(mesh.https_port))),
        ("stun_listen_addr", s(&format!(":{STUN_LISTEN_PORT}"))),
        ("stun_public_addr", s(&public(mesh.stun_port))),
        (
            "tls",
            map(vec![
                ("disable", Value::from(false)),
                ("cert_file", s(&in_container(CERT_FILE))),
                ("key_file", s(&in_container(KEY_FILE))),
            ]),
        ),
        (
            "database",
            map(vec![
                ("type", s("sqlite")),
                (
                    "url",
                    s(&format!(
                        "{DATA_DIR}/ionscale.db?_pragma=busy_timeout(5000)\
                         &_pragma=journal_mode(WAL)&_pragma=foreign_keys(ON)"
                    )),
                ),
            ]),
        ),
        (
            "auth",
            map(vec![(
                "service_tokens",
                list(vec![map(vec![
                    ("name", s("lok")),
                    ("token", s(&mesh.service_token)),
                ])]),
            )]),
        ),
        ("dns", map(vec![("magic_dns_suffix", s(&mesh.dns_suffix))])),
        ("logging", map(vec![("level", s("info"))])),
    ])
}

/// ionskale's compose service: both ports published, since nodes outside the stack are
/// told the address of this machine. Nothing it needs is in the stack, so it starts first.
pub fn ionskale_service(mesh: &LocalMeshBlock) -> Value {
    map(vec![
        ("image", s(&mesh.image)),
        (
            "command",
            list(vec![
                s("server"),
                s("--config"),
                s("/etc/ionscale/config.yaml"),
            ]),
        ),
        (
            "ports",
            list(vec![
                s(&format!("{}:{LISTEN_PORT}", mesh.https_port)),
                s(&format!("{}:{STUN_LISTEN_PORT}/udp", mesh.stun_port)),
            ]),
        ),
        (
            "volumes",
            list(vec![
                s(&format!("./{CONFIG_FILE}:/etc/ionscale/config.yaml:ro")),
                s(&format!("./{}:{CONTAINER_TLS_DIR}:ro", tls_dir())),
                s(&format!("{DATA_VOLUME}:{DATA_DIR}")),
            ]),
        ),
        ("stop_grace_period", s("2s")),
        ("restart", s("on-failure")),
    ])
}

fn tls_dir() -> &'static str {
    CA_FILE
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or(CA_FILE)
}

/// What the hub's sidecar is given beside what every sidecar is: the CA to trust its
/// control server with (Go reads `SSL_CERT_FILE` instead of the system's roots, which it
/// has no use for here), and the volume Lok writes its key to.
pub fn sidecar_environment() -> Vec<(&'static str, String)> {
    vec![("SSL_CERT_FILE", in_container(CA_FILE))]
}

pub fn sidecar_volumes() -> Vec<String> {
    vec![
        format!("./{CA_FILE}:{}:ro", in_container(CA_FILE)),
        format!(
            "{}:{}:ro",
            crate::config::local_mesh::KEYS_VOLUME,
            crate::config::local_mesh::KEYS_DIR
        ),
    ]
}

/// Lok's `ionscale:` block. It reaches ionskale on the stack's network, under a name the
/// certificate carries but a CA it was never given: it is told not to check.
pub fn lok_ionscale(mesh: &LocalMeshBlock, hub_identifier: &str) -> Value {
    map(vec![
        ("server_url", s(&mesh.server_url())),
        ("verify_tls", Value::from(false)),
        ("coord_url", s(&mesh.coord_url())),
        ("service_token", s(&mesh.service_token)),
        ("magic_dns_suffix", s(&mesh.dns_suffix)),
        ("auto_create_mesh", Value::from(true)),
        // The one hub whose node this deployment runs: its key, minted while Lok is
        // prepared, goes where the sidecar reads it.
        (
            "hub_key_files",
            map(vec![(hub_identifier, s(HUB_KEY_FILE))]),
        ),
    ])
}

/// What `secrets/access.json` says of the mesh: where nodes join, and what they trust.
pub fn access(mesh: &LocalMeshBlock) -> serde_json::Value {
    serde_json::json!({
        "coord_url": mesh.coord_url(),
        "ca_file": CA_FILE,
    })
}
