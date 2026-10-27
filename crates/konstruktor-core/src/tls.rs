//! The hub's TLS certificate: one self-signed certificate for every host the hub is reached at.
//!
//! A hub with `gateway.ssl` serves its gateway (and LiveKit's signalling) over HTTPS with this
//! certificate, and sends it — it is public — to the coordination server when it is authorized
//! (`HubManifest.tls_certificate`). The coordination server hands it to every app on the hub's
//! instances, and an app that knows it trusts exactly this certificate for the hub's aliases:
//! there is no CA and no chain. The app can believe it because it got it from the coordination
//! server, which is reached over a publicly valid certificate.
//!
//! It names every advertised host, `localhost`, the loopback addresses and the gateway's name on
//! the hub's own network. Not the tailnet name, which only the coordination server learns: on
//! the mesh the hub is reached in plain HTTP, the mesh itself being encrypted.
//!
//! It is generated when the hub is authorized and the hosts it must name changed (or it has
//! none yet); an unchanged hub keeps it, so a re-authorization for a service change does not
//! make every app fetch a new one.

use rcgen::{
    date_time_ymd, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose, SanType,
};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// How long a certificate is valid. Long: rotating it is a re-authorization, and nothing but
/// this hub's apps ever trusts it.
pub const VALID_YEARS: i32 = 20;

/// Where the gateway container finds the certificate and its key.
pub const CONTAINER_DIR: &str = "/etc/caddy/tls";
/// Where the certificate and its key are written in the hub's folder.
pub const FOLDER_DIR: &str = "configs/gateway";
pub const CERT_FILE: &str = "hub.crt";
pub const KEY_FILE: &str = "hub.key";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayCertificate {
    /// The hosts it names, sorted: what decides whether it is still the right one.
    pub hosts: Vec<String>,
    pub cert_pem: String,
    /// The private key. Secret, like the rest of the profile's keys.
    pub key_pem: String,
}

/// Every host the certificate must name for a hub advertised at `advertised`, whose gateway is
/// `gateway_host` on its own network. Sorted and deduplicated, lowercase.
pub fn certificate_hosts(advertised: &[String], gateway_host: &str) -> Vec<String> {
    let mut hosts: Vec<String> = advertised
        .iter()
        .map(String::as_str)
        .chain(["localhost", "127.0.0.1", "::1", gateway_host])
        .map(|host| {
            host.trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_lowercase()
        })
        .filter(|host| !host.is_empty())
        .collect();
    hosts.sort();
    hosts.dedup();
    hosts
}

/// A fresh self-signed server certificate (ECDSA P-256) naming `hosts`.
///
/// A server certificate, not a CA: rustls — what LiveKit's SDKs verify with — refuses a CA
/// certificate presented as a server's, and so does the coordination server.
pub fn generate(hosts: &[String], current_year: i32) -> Result<GatewayCertificate, String> {
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "Arkitekt hub");
    params.distinguished_name = name;
    params.subject_alt_names = hosts
        .iter()
        .map(|host| match host.parse::<IpAddr>() {
            Ok(ip) => Ok(SanType::IpAddress(ip)),
            Err(_) if !is_dns_name(host) => Err(format!("{host} cannot be named in a certificate")),
            Err(_) => host
                .clone()
                .try_into()
                .map(SanType::DnsName)
                .map_err(|e| format!("{host} cannot be named in a certificate: {e}")),
        })
        .collect::<Result<_, _>>()?;
    // No basicConstraints at all: an end-entity certificate. `ExplicitNoCa` would encode
    // `CA:FALSE`, a DEFAULT value DER forbids writing — strict parsers (Python's
    // `cryptography`, which the coordination server validates with) refuse the certificate.
    params.is_ca = IsCa::NoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.not_before = date_time_ymd(current_year - 1, 1, 1);
    params.not_after = date_time_ymd(current_year + VALID_YEARS, 1, 1);

    let key = KeyPair::generate().map_err(|e| format!("could not make the gateway's key: {e}"))?;
    let certificate = params
        .self_signed(&key)
        .map_err(|e| format!("could not make the gateway's certificate: {e}"))?;
    Ok(GatewayCertificate {
        hosts: hosts.to_vec(),
        cert_pem: certificate.pem(),
        key_pem: key.serialize_pem(),
    })
}

/// Letters, digits, `-` and `_` (docker service names), in dot-separated labels.
fn is_dns_name(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}

/// This year, by the system clock. Only for the validity window, which has a year to spare on
/// either side.
pub fn current_year() -> i32 {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    1970 + (seconds / 31_556_952) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_advertised_hosts_and_the_local_ones_once() {
        let hosts = certificate_hosts(
            &["Lab.Local".into(), "10.0.0.4".into(), "localhost".into()],
            "gateway",
        );
        assert_eq!(
            hosts,
            [
                "10.0.0.4",
                "127.0.0.1",
                "::1",
                "gateway",
                "lab.local",
                "localhost"
            ]
        );
    }

    #[test]
    fn makes_a_server_certificate_for_names_and_addresses() {
        let hosts = certificate_hosts(&["lab.local".into(), "10.0.0.4".into()], "gateway");
        let certificate = generate(&hosts, 2026).unwrap();
        assert!(certificate
            .cert_pem
            .starts_with("-----BEGIN CERTIFICATE-----"));
        assert!(certificate.key_pem.contains("PRIVATE KEY"));
        assert_eq!(certificate.hosts, hosts);
        // Each call is a new key: a rotation is a new certificate.
        assert_ne!(
            generate(&hosts, 2026).unwrap().cert_pem,
            certificate.cert_pem
        );
    }

    /// DER with no DEFAULT value encoded: no basicConstraints extension at all (its OID,
    /// 2.5.29.19, never appears), which is what strict parsers require.
    #[test]
    fn writes_no_basic_constraints() {
        let certificate = generate(&["lab.local".into()], 2026).unwrap();
        use base64::Engine;
        let body: String = certificate
            .cert_pem
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();
        let der = base64::engine::general_purpose::STANDARD
            .decode(body)
            .unwrap();
        let oid = [0x06, 0x03, 0x55, 0x1d, 0x13];
        assert!(!der.windows(oid.len()).any(|w| w == oid));
    }

    #[test]
    fn refuses_a_host_no_certificate_can_name() {
        assert!(generate(&["not a host".into()], 2026).is_err());
    }
}
