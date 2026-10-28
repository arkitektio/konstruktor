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

/// Where a public site's certificate and key are put in the folder, beside the self-signed
/// one, when somebody provides them (see [`crate::config::hub::PublicTls::Provided`]): copied
/// from where they lie, so a renewal that swaps a symlink is picked up by copying again
/// rather than by recreating the gateway.
pub const PUBLIC_CERT_FILE: &str = "public.crt";
pub const PUBLIC_KEY_FILE: &str = "public.key";
/// The root of a private ACME CA, copied the same way.
pub const ACME_ROOT_FILE: &str = "acme-root.pem";

/// A certificate is worth a warning when it has fewer days left than this: Let's Encrypt
/// renews at thirty, and a provided one renewed by hand needs time to be.
pub const RENEWAL_WARNING_DAYS: i64 = 21;

/// What a certificate somebody provides was found to be, checked as a client checks it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CertificateInfo {
    /// The names it was checked for: it is valid for each.
    pub names: Vec<String>,
    /// When it stops being valid, in seconds since the epoch.
    pub not_after: i64,
}

impl CertificateInfo {
    /// Whole days left at `now` (seconds since the epoch); negative once it expired.
    pub fn days_left(&self, now: i64) -> i64 {
        (self.not_after - now).div_euclid(86_400)
    }

    pub fn expires_soon(&self, now: i64) -> bool {
        self.days_left(now) < RENEWAL_WARNING_DAYS
    }
}

/// Checks a certificate and its chain for `names`, the way a client of the site will: that
/// it chains to a root `roots` trusts, is valid now, and names each of `names` (a wildcard
/// counts as a client counts it). With `key_pem`, also that the key is the certificate's.
///
/// What it does not check is that the site is reachable under those names: that is the DNS
/// preflight's, and [`crate::ready`]'s once it runs.
pub fn inspect(
    cert_pem: &[u8],
    key_pem: Option<&[u8]>,
    names: &[String],
    roots: rustls::RootCertStore,
    now: std::time::SystemTime,
) -> Result<CertificateInfo, String> {
    use rustls::client::danger::ServerCertVerifier;
    use rustls::pki_types::{pem::PemObject, PrivateKeyDer, ServerName, UnixTime};

    let chain = crate::trust::parse_certificates(cert_pem)?;
    let Some((leaf, intermediates)) = chain.split_first() else {
        return Err("the certificate file holds no PEM certificate".into());
    };

    if let Some(key_pem) = key_pem {
        let key = PrivateKeyDer::from_pem_slice(key_pem)
            .map_err(|e| format!("the key file holds no readable PEM private key: {e}"))?;
        let signer = rustls::crypto::ring::sign::any_supported_type(&key)
            .map_err(|e| format!("the private key is of a kind that cannot be served: {e}"))?;
        rustls::sign::CertifiedKey::new(chain.clone(), signer)
            .keys_match()
            .map_err(|e| match e {
                rustls::Error::InconsistentKeys(rustls::InconsistentKeys::KeyMismatch) => {
                    "the private key is not the certificate's".to_string()
                }
                other => format!("the private key cannot be matched to the certificate: {other}"),
            })?;
    }

    let verifier = crate::trust::verifier(roots)?;
    let at = UnixTime::since_unix_epoch(
        now.duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default(),
    );
    for name in names {
        let server = ServerName::try_from(name.as_str())
            .map_err(|_| format!("{name} cannot be named in a certificate"))?;
        verifier
            .verify_server_cert(leaf, intermediates, &server, &[], at)
            .map_err(|e| describe_refusal(name, &e))?;
    }

    let not_after = validity(leaf.as_ref())
        .map(|(_, not_after)| not_after)
        .ok_or("the certificate's validity cannot be read")?;
    Ok(CertificateInfo {
        names: names.to_vec(),
        not_after,
    })
}

/// What a client would say when it refuses `name`'s certificate, in words that say what to do.
fn describe_refusal(name: &str, error: &rustls::Error) -> String {
    use rustls::{CertificateError, Error};
    match error {
        Error::InvalidCertificate(CertificateError::NotValidForName)
        | Error::InvalidCertificate(CertificateError::NotValidForNameContext { .. }) => {
            format!("the certificate does not name {name}")
        }
        Error::InvalidCertificate(CertificateError::Expired)
        | Error::InvalidCertificate(CertificateError::ExpiredContext { .. }) => {
            "the certificate has expired".to_string()
        }
        Error::InvalidCertificate(CertificateError::NotValidYet)
        | Error::InvalidCertificate(CertificateError::NotValidYetContext { .. }) => {
            "the certificate is not valid yet".to_string()
        }
        // A bad signature is the same answer: a root of that name is trusted, and it is not
        // the one that signed this.
        Error::InvalidCertificate(CertificateError::UnknownIssuer)
        | Error::InvalidCertificate(CertificateError::BadSignature) => format!(
            "the certificate does not chain to a root a client trusts: put the whole chain in \
             the certificate file, leaf first — or, for a CA of your own, name its root in \
             {}",
            crate::trust::CA_FILE_ENV
        ),
        other => format!("the certificate is refused for {name}: {other}"),
    }
}

/// Puts a public site's files where the gateway reads them, in `dir`: a provided certificate
/// and its key, checked first ([`inspect`]), and a private ACME CA's root. Returns what the
/// provided certificate was found to be.
///
/// Copied rather than mounted from where they lie: certbot's `live/` files are symlinks a
/// renewal points elsewhere, and a mount keeps the file it was made with. Run again, it
/// picks up a renewal — `konstruktor tls reload`. Each file is replaced in one rename, so the
/// gateway never reads half of one.
pub fn install_public_files(
    dir: &std::path::Path,
    config: &crate::config::hub::HubConfig,
) -> Result<Option<CertificateInfo>, String> {
    use crate::config::hub::PublicTls;
    let Some(site) = config.public_site() else {
        return Ok(None);
    };
    let folder = dir.join(FOLDER_DIR);
    match &site.tls {
        PublicTls::Provided {
            cert_path,
            key_path,
        } => {
            let cert = std::fs::read(cert_path).map_err(|e| format!("{cert_path}: {e}"))?;
            let key = std::fs::read(key_path).map_err(|e| format!("{key_path}: {e}"))?;
            let info = inspect(
                &cert,
                Some(&key),
                &site.names(),
                crate::trust::root_store()?,
                std::time::SystemTime::now(),
            )?;
            replace(&folder.join(PUBLIC_CERT_FILE), &cert, false)?;
            replace(&folder.join(PUBLIC_KEY_FILE), &key, true)?;
            Ok(Some(info))
        }
        PublicTls::Acme { ca_root, .. } => {
            if let Some(root) = ca_root {
                let pem = std::fs::read(root).map_err(|e| format!("{root}: {e}"))?;
                crate::trust::parse_certificates(&pem).map_err(|e| format!("{root}: {e}"))?;
                replace(&folder.join(ACME_ROOT_FILE), &pem, false)?;
            }
            Ok(None)
        }
    }
}

/// What the certificate a public site is served with now says, from the folder: a provided
/// one's copy. `None` for an ACME site, whose certificate is the gateway's.
pub fn installed_certificate(
    dir: &std::path::Path,
    config: &crate::config::hub::HubConfig,
) -> Option<Result<CertificateInfo, String>> {
    use crate::config::hub::PublicTls;
    let site = config.public_site()?;
    let PublicTls::Provided { .. } = site.tls else {
        return None;
    };
    let folder = dir.join(FOLDER_DIR);
    Some((|| {
        let cert = std::fs::read(folder.join(PUBLIC_CERT_FILE))
            .map_err(|e| format!("the certificate in the folder: {e}"))?;
        inspect(
            &cert,
            None,
            &site.names(),
            crate::trust::root_store()?,
            std::time::SystemTime::now(),
        )
    })())
}

/// Writes `contents` to `path` through a file beside it, renamed into place.
fn replace(path: &std::path::Path, contents: &[u8], secret: bool) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("no folder to write the certificate in")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let mut staged = path.as_os_str().to_owned();
    staged.push(".partial");
    let staged = std::path::PathBuf::from(staged);
    std::fs::write(&staged, contents).map_err(|e| format!("{}: {e}", staged.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if secret { 0o600 } else { 0o644 };
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(mode))
            .map_err(|e| format!("{}: {e}", staged.display()))?;
    }
    #[cfg(not(unix))]
    let _ = secret;
    std::fs::rename(&staged, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// `(notBefore, notAfter)` of a DER certificate, in seconds since the epoch.
///
/// Only as far into the structure as these two lie: Certificate → TBSCertificate → (an
/// explicit version, the serial, the signature algorithm, the issuer) → Validity.
pub fn validity(der: &[u8]) -> Option<(i64, i64)> {
    let (_, certificate, _) = tlv(der)?;
    let (_, mut tbs, _) = tlv(certificate)?;
    // [0] EXPLICIT version, only present on v2 and v3 certificates.
    if tbs.first() == Some(&0xa0) {
        tbs = tlv(tbs)?.2;
    }
    // serialNumber, signature, issuer.
    for _ in 0..3 {
        tbs = tlv(tbs)?.2;
    }
    let (tag, validity, _) = tlv(tbs)?;
    if tag != 0x30 {
        return None;
    }
    let (before_tag, before, rest) = tlv(validity)?;
    let (after_tag, after, _) = tlv(rest)?;
    Some((asn1_time(before_tag, before)?, asn1_time(after_tag, after)?))
}

/// One DER tag-length-value: `(tag, contents, what follows)`.
fn tlv(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (length, rest) = if first < 0x80 {
        (first as usize, rest)
    } else {
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let length = rest[..count]
            .iter()
            .fold(0usize, |acc, byte| (acc << 8) | *byte as usize);
        (length, &rest[count..])
    };
    (rest.len() >= length).then(|| (tag, &rest[..length], &rest[length..]))
}

/// A UTCTime (`YYMMDDHHMMSSZ`, tag 0x17) or GeneralizedTime (`YYYYMMDDHHMMSSZ`, tag 0x18), as
/// RFC 5280 writes them, in seconds since the epoch.
fn asn1_time(tag: u8, value: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(value).ok()?.strip_suffix('Z')?;
    let (year, rest) = match tag {
        0x17 if text.len() == 12 => {
            let yy: i64 = text[..2].parse().ok()?;
            (if yy >= 50 { 1900 + yy } else { 2000 + yy }, &text[2..])
        }
        0x18 if text.len() == 14 => (text[..4].parse().ok()?, &text[4..]),
        _ => return None,
    };
    let field = |i: usize| rest.get(i..i + 2)?.parse::<i64>().ok();
    let (month, day, hour, minute, second) =
        (field(0)?, field(2)?, field(4)?, field(6)?, field(8)?);
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Seconds since the epoch, by the system clock.
pub fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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

    /// A CA of one's own and a leaf it signs for `names`, valid in `years` (from, to).
    struct Issued {
        ca_pem: String,
        cert_pem: String,
        key_pem: String,
    }

    fn issue(names: &[&str], years: (i32, i32)) -> Issued {
        let ca_key = KeyPair::generate().unwrap();
        let mut ca = CertificateParams::default();
        ca.distinguished_name.push(DnType::CommonName, "Test CA");
        ca.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca_cert = ca.self_signed(&ca_key).unwrap();

        let key = KeyPair::generate().unwrap();
        let mut leaf =
            CertificateParams::new(names.iter().map(|n| n.to_string()).collect::<Vec<_>>())
                .unwrap();
        leaf.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        leaf.not_before = date_time_ymd(years.0, 1, 1);
        leaf.not_after = date_time_ymd(years.1, 1, 1);
        let cert = leaf.signed_by(&key, &ca_cert, &ca_key).unwrap();
        Issued {
            ca_pem: ca_cert.pem(),
            cert_pem: cert.pem(),
            key_pem: key.serialize_pem(),
        }
    }

    fn roots(ca_pem: &str) -> rustls::RootCertStore {
        let mut store = rustls::RootCertStore::empty();
        for root in crate::trust::parse_certificates(ca_pem.as_bytes()).unwrap() {
            store.add(root).unwrap();
        }
        store
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn a_provided_certificate_is_checked_for_its_names_its_key_and_its_dates() {
        let issued = issue(&["coord.example.org", "mesh.example.org"], (2020, 2040));
        let info = inspect(
            issued.cert_pem.as_bytes(),
            Some(issued.key_pem.as_bytes()),
            &names(&["coord.example.org", "mesh.example.org"]),
            roots(&issued.ca_pem),
            std::time::SystemTime::now(),
        )
        .unwrap();
        // 2040-01-01T00:00:00Z.
        assert_eq!(info.not_after, 2_208_988_800);
        assert!(!info.expires_soon(now_seconds()));
    }

    #[test]
    fn a_wildcard_covers_a_name_below_it() {
        let issued = issue(&["*.example.org"], (2020, 2040));
        inspect(
            issued.cert_pem.as_bytes(),
            None,
            &names(&["coord.example.org"]),
            roots(&issued.ca_pem),
            std::time::SystemTime::now(),
        )
        .unwrap();
    }

    #[test]
    fn a_name_the_certificate_does_not_cover_is_refused_by_name() {
        let issued = issue(&["coord.example.org"], (2020, 2040));
        let error = inspect(
            issued.cert_pem.as_bytes(),
            None,
            &names(&["coord.example.org", "mesh.example.org"]),
            roots(&issued.ca_pem),
            std::time::SystemTime::now(),
        )
        .unwrap_err();
        assert!(error.contains("does not name mesh.example.org"), "{error}");
    }

    #[test]
    fn another_certificates_key_is_refused() {
        let issued = issue(&["coord.example.org"], (2020, 2040));
        let other = issue(&["coord.example.org"], (2020, 2040));
        let error = inspect(
            issued.cert_pem.as_bytes(),
            Some(other.key_pem.as_bytes()),
            &names(&["coord.example.org"]),
            roots(&issued.ca_pem),
            std::time::SystemTime::now(),
        )
        .unwrap_err();
        assert!(error.contains("not the certificate's"), "{error}");
    }

    #[test]
    fn an_expired_certificate_is_refused() {
        let issued = issue(&["coord.example.org"], (2010, 2015));
        let error = inspect(
            issued.cert_pem.as_bytes(),
            None,
            &names(&["coord.example.org"]),
            roots(&issued.ca_pem),
            std::time::SystemTime::now(),
        )
        .unwrap_err();
        assert!(error.contains("expired"), "{error}");
    }

    #[test]
    fn a_certificate_of_an_unknown_ca_says_how_to_trust_it() {
        let issued = issue(&["coord.example.org"], (2020, 2040));
        let stranger = issue(&["coord.example.org"], (2020, 2040));
        let error = inspect(
            issued.cert_pem.as_bytes(),
            None,
            &names(&["coord.example.org"]),
            roots(&stranger.ca_pem),
            std::time::SystemTime::now(),
        )
        .unwrap_err();
        assert!(error.contains(crate::trust::CA_FILE_ENV), "{error}");
    }

    #[test]
    fn one_expiring_soon_is_said_to() {
        let info = CertificateInfo {
            names: names(&["coord.example.org"]),
            not_after: 1_000 * 86_400,
        };
        assert_eq!(info.days_left(990 * 86_400), 10);
        assert!(info.expires_soon(990 * 86_400));
        assert!(!info.expires_soon(900 * 86_400));
    }

    #[test]
    fn reads_the_validity_of_the_self_signed_certificate() {
        let certificate = generate(&["lab.local".into()], 2026).unwrap();
        let der = crate::trust::parse_certificates(certificate.cert_pem.as_bytes()).unwrap();
        let (not_before, not_after) = validity(der[0].as_ref()).unwrap();
        // 2025-01-01 and 2046-01-01, both at midnight UTC.
        assert_eq!(not_before, 1_735_689_600);
        assert_eq!(not_after, 2_398_377_600);
    }
}
