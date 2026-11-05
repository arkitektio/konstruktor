//! A self-contained hub's own mesh: an ionskale beside its Lok, and the hub on it.
//!
//! A hub accepted by a coordination server joins that server's mesh. A self-contained hub
//! has no such server: it runs its own Lok, and with `--mesh local` its own mesh control
//! server too, so that an app reaches its services the way it would reach a hub that is
//! only on a mesh. It exists for that — proving, on one machine, that an app gets to a
//! service over the mesh and nothing else.
//!
//! Nodes reach ionskale at [`LocalMeshBlock::address`] on two published ports: control and
//! relay (DERP, embedded) over TLS on one, STUN on the other. It is an address both the host
//! and every container reach — the docker bridge's — so the hub's sidecar, an app on this
//! machine and an app in a container of its own all join the same way. Tailscale wants TLS
//! for both, so ionskale serves it itself, with a certificate from a CA minted here that only
//! this hub's nodes are given ([`LocalMeshBlock::ca_pem`]).
//!
//! The hub's own node is the [`crate::config::mesh::MeshBlock`] sidecar every hub on a mesh
//! has. Its key is no secret known up front: Lok mints it for the hub while it is prepared
//! (`ionscale.hub_key_files`) and writes it to [`HUB_KEY_FILE`], on a volume the sidecar
//! reads it from.

use rcgen::{
    date_time_ymd, BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose,
};
use serde::{Deserialize, Serialize};

use crate::config::mesh::{build_mesh_block, mesh_hostname, MeshBlock, MeshOptions};
use crate::secrets::generate_alpha_numeric_string;

/// The mesh control server's compose service.
pub const IONSKALE_HOST: &str = "ionskale";
/// What it listens on inside its container: control and relay over TLS.
pub const LISTEN_PORT: u16 = 443;
/// Its STUN server, inside its container.
pub const STUN_LISTEN_PORT: u16 = 3478;

/// Its database (SQLite) lives on this volume.
pub const DATA_VOLUME: &str = "ionskale_data";
pub const DATA_DIR: &str = "/data/ionscale";

/// Where Lok writes the hub's key and the sidecar reads it.
pub const KEYS_VOLUME: &str = "mesh_keys";
pub const KEYS_DIR: &str = "/mesh";
pub const HUB_KEY_FILE: &str = "/mesh/hub.key";

/// Its config, certificate and key, in the folder. Not under `secrets/`: ionskale runs as
/// an unprivileged user, which a file only its owner reads would lock out. The key secures
/// nothing but this machine's own mesh control plane, under a CA nobody else trusts.
pub const CONFIG_FILE: &str = "configs/ionskale/config.yaml";
pub const CA_FILE: &str = "configs/ionskale/tls/ca.pem";
pub const CERT_FILE: &str = "configs/ionskale/tls/server.pem";
pub const KEY_FILE: &str = "configs/ionskale/tls/server.key";
/// Where the TLS directory is mounted in ionskale's container, and in the sidecar's.
pub const CONTAINER_TLS_DIR: &str = "/etc/ionscale/tls";

/// The MagicDNS suffix of its tailnets. Never a real domain.
pub const DNS_SUFFIX: &str = "mesh.internal";

/// How long the CA and certificate are valid: as long as a hub is likely kept.
const VALID_YEARS: i32 = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalMeshBlock {
    pub host: String,
    pub image: String,
    /// Where nodes reach it: an address of this machine that its containers reach too.
    pub address: String,
    /// The published port of control and relay.
    pub https_port: u16,
    /// The published port of STUN, the same inside and out: clients are told it.
    pub stun_port: u16,
    pub dns_suffix: String,
    /// The bearer token Lok drives it with (`svc_…`).
    pub service_token: String,
    /// The CA its certificate is signed by: what a node is told to trust.
    pub ca_pem: String,
    pub cert_pem: String,
    /// The certificate's private key.
    pub key_pem: String,
}

impl LocalMeshBlock {
    /// What a node is given as its control server.
    pub fn coord_url(&self) -> String {
        let host = if self.address.contains(':') {
            format!("[{}]", self.address)
        } else {
            self.address.clone()
        };
        format!("https://{host}:{}", self.https_port)
    }

    /// Where Lok drives it, on the stack's own network.
    pub fn server_url(&self) -> String {
        format!("https://{}:{LISTEN_PORT}", self.host)
    }
}

/// What a local mesh is made with.
#[derive(Debug, Clone)]
pub struct LocalMeshOptions {
    pub image: String,
    pub address: String,
    pub https_port: u16,
    pub stun_port: u16,
}

pub fn build_local_mesh_block(
    options: &LocalMeshOptions,
    current_year: i32,
) -> Result<LocalMeshBlock, String> {
    let address = options.address.trim().to_string();
    if address.is_empty() {
        return Err("A local mesh needs an address its nodes reach it at.".into());
    }
    let (ca_pem, cert_pem, key_pem) = mint(&address, current_year)?;
    Ok(LocalMeshBlock {
        host: IONSKALE_HOST.to_string(),
        image: options.image.clone(),
        address,
        https_port: options.https_port,
        stun_port: options.stun_port,
        dns_suffix: DNS_SUFFIX.to_string(),
        service_token: format!("svc_{}", generate_alpha_numeric_string(40)),
        ca_pem,
        cert_pem,
        key_pem,
    })
}

/// The sidecar that puts the hub's gateway on its own mesh: the [`MeshBlock`] every hub on a
/// mesh has, logging in to ionskale with the key Lok writes to [`HUB_KEY_FILE`] — named as
/// `tailscale up` reads a key from a file, so `mesh.env` holds no secret.
pub fn hub_sidecar(identifier: &str, mesh: &LocalMeshBlock) -> MeshBlock {
    build_mesh_block(&MeshOptions {
        hostname: mesh_hostname(identifier),
        auth_key: format!("file:{HUB_KEY_FILE}"),
        coord_url: Some(mesh.coord_url()),
        login: None,
    })
}

/// A CA, and a server certificate it signs for every name ionskale is reached at: its
/// address, its name on the stack's network, and this machine's loopback.
///
/// A CA rather than one self-signed certificate: Tailscale's control client and the mesh
/// bindings each take a root to trust, not a certificate to pin.
fn mint(address: &str, current_year: i32) -> Result<(String, String, String), String> {
    let failed = |e: rcgen::Error| format!("could not make the local mesh's certificate: {e}");

    let ca_key = KeyPair::generate().map_err(failed)?;
    let mut ca = CertificateParams::default();
    ca.distinguished_name
        .push(DnType::CommonName, "Arkitekt hub mesh CA");
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    ca.not_before = date_time_ymd(current_year - 1, 1, 1);
    ca.not_after = date_time_ymd(current_year + VALID_YEARS, 1, 1);
    let ca_cert = ca.self_signed(&ca_key).map_err(failed)?;

    let mut names = vec![
        IONSKALE_HOST.to_string(),
        "localhost".to_string(),
        "127.0.0.1".to_string(),
    ];
    if !names.iter().any(|n| n == address) {
        names.push(address.to_string());
    }
    let key = KeyPair::generate().map_err(failed)?;
    let mut leaf = CertificateParams::new(names).map_err(failed)?;
    leaf.distinguished_name
        .push(DnType::CommonName, IONSKALE_HOST);
    leaf.is_ca = IsCa::NoCa;
    leaf.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    // Strict verifiers (Python 3.13's default context among them) want the chain linked by
    // key identifiers.
    leaf.use_authority_key_identifier_extension = true;
    leaf.not_before = date_time_ymd(current_year - 1, 1, 1);
    leaf.not_after = date_time_ymd(current_year + VALID_YEARS, 1, 1);
    let cert = leaf.signed_by(&key, &ca_cert, &ca_key).map_err(failed)?;

    Ok((ca_cert.pem(), cert.pem(), key.serialize_pem()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(address: &str) -> LocalMeshBlock {
        build_local_mesh_block(
            &LocalMeshOptions {
                image: "jhnnsrs/ionskale:latest".into(),
                address: address.into(),
                https_port: 41443,
                stun_port: 41478,
            },
            2026,
        )
        .unwrap()
    }

    /// The certificate verifies against the CA for the address nodes are told, as a
    /// Tailscale client checks it.
    #[test]
    fn its_certificate_is_valid_for_the_address_nodes_dial() {
        let mesh = block("172.17.0.1");
        assert_eq!(mesh.coord_url(), "https://172.17.0.1:41443");

        let mut roots = rustls::RootCertStore::empty();
        for root in crate::trust::parse_certificates(mesh.ca_pem.as_bytes()).unwrap() {
            roots.add(root).unwrap();
        }
        crate::tls::inspect(
            mesh.cert_pem.as_bytes(),
            Some(mesh.key_pem.as_bytes()),
            &["172.17.0.1".to_string(), "ionskale".to_string()],
            roots,
            std::time::SystemTime::now(),
        )
        .unwrap();
    }
}
