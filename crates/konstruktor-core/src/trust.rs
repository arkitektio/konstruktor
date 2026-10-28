//! What Konstruktor trusts when it speaks TLS: the web PKI's roots, this machine's own, and
//! any root it is told of.
//!
//! A coordination server on the internet has a certificate from a public CA, which the
//! bundled roots cover. One behind an institution's CA is trusted where this machine trusts
//! that CA, as a browser on it would. And a CA nobody else trusts — a lab's step-ca, the
//! Pebble an ACME test runs against — is named in [`CA_FILE_ENV`]: a PEM file of roots,
//! trusted beside the others and never instead of them.
//!
//! Everything that verifies a peer asks here — the HTTP client every request to a
//! coordination server goes through ([`client_builder`]), and the check of a certificate
//! somebody provides for a gateway ([`crate::tls::inspect`]) — so the two cannot disagree
//! about what is trusted.

use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;

/// A PEM file of further roots to trust.
pub const CA_FILE_ENV: &str = "KONSTRUKTOR_CA_FILE";

/// The roots [`CA_FILE_ENV`] names, as given. Empty when it names none; an error when it
/// names a file that cannot be read or holds no certificate, since a root that was asked for
/// and silently left out would show up as a refusal much later, somewhere else.
pub fn extra_roots() -> Result<Vec<CertificateDer<'static>>, String> {
    let Some(path) = std::env::var_os(CA_FILE_ENV).filter(|p| !p.is_empty()) else {
        return Ok(Vec::new());
    };
    roots_in(std::path::Path::new(&path)).map_err(|e| format!("{CA_FILE_ENV} names {e}"))
}

/// The certificates of the PEM file at `path`, as roots: at least one.
pub fn roots_in(path: &std::path::Path) -> Result<Vec<CertificateDer<'static>>, String> {
    let pem = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let roots = parse_certificates(&pem).map_err(|e| format!("{}: {e}", path.display()))?;
    if roots.is_empty() {
        return Err(format!(
            "{}, which holds no PEM certificate",
            path.display()
        ));
    }
    Ok(roots)
}

/// Every certificate in a PEM document, in order.
pub fn parse_certificates(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>, String> {
    CertificateDer::pem_slice_iter(pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("not a readable PEM certificate: {e}"))
}

/// The web PKI's roots, this machine's, and [`extra_roots`].
///
/// A machine's store that cannot be read is not fatal — the bundled roots still stand — but
/// one root in it that does not parse is skipped rather than failing the rest.
pub fn root_store() -> Result<rustls::RootCertStore, String> {
    root_store_with(extra_roots()?)
}

/// The web PKI's roots, this machine's, and `extra`.
pub fn root_store_with(
    extra: Vec<CertificateDer<'static>>,
) -> Result<rustls::RootCertStore, String> {
    let mut store = rustls::RootCertStore::empty();
    store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    for root in rustls_native_certs::load_native_certs().certs {
        let _ = store.add(root);
    }
    for root in extra {
        store
            .add(root)
            .map_err(|e| format!("a root that was named cannot be trusted: {e}"))?;
    }
    Ok(store)
}

/// The verifier a client of a server named in `store` would check that server with.
pub fn verifier(
    store: rustls::RootCertStore,
) -> Result<Arc<rustls::client::WebPkiServerVerifier>, String> {
    rustls::client::WebPkiServerVerifier::builder_with_provider(
        Arc::new(store),
        Arc::new(rustls::crypto::ring::default_provider()),
    )
    .build()
    .map_err(|e| format!("no roots to verify against: {e}"))
}

/// A `reqwest` client builder that trusts what this module trusts. The machine's roots come
/// with reqwest's own `rustls-tls-native-roots`; the extra ones are added here.
///
/// A root [`CA_FILE_ENV`] names that cannot be read is reported on stderr and left out, so a
/// request that does not need it still goes through.
pub fn client_builder() -> reqwest::ClientBuilder {
    let mut builder = reqwest::Client::builder();
    match extra_roots() {
        Ok(roots) => {
            for root in roots {
                match reqwest::Certificate::from_der(root.as_ref()) {
                    Ok(certificate) => builder = builder.add_root_certificate(certificate),
                    Err(e) => eprintln!("{CA_FILE_ENV}: a root is left out: {e}"),
                }
            }
        }
        Err(e) => eprintln!("{e}; it is left out"),
    }
    builder
}

/// [`client_builder`], built.
pub fn client() -> reqwest::Client {
    client_builder()
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}
