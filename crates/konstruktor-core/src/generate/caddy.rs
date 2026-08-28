use std::fmt::Write as _;

use crate::catalog::{ServiceId, HUB_SERVICE_ORDER};

/// The gateway config, and the only generated file compared byte-for-byte against what
/// the Python generator writes.
///
/// Its whitespace is asymmetric in a way no formatter would guess: tabs at both levels,
/// and a **trailing space after the opening brace** on every service and bucket handler —
/// but *not* on the trailing minio block, which upstream emits from a different code path.
/// Both forms are written here as explicit escapes so an editor that trims trailing
/// whitespace cannot silently break the comparison.
const OPEN_BRACE_WITH_SPACE: &str = "{ ";
const OPEN_BRACE_BARE: &str = "{";

/// What the Caddyfile needs to know about one routed upstream.
pub struct Upstream<'a> {
    /// The matcher name and the path prefix — `@mikro path /mikro*`.
    pub name: &'a str,
    pub target: &'a str,
    pub port: u16,
}

fn route(out: &mut String, name: &str, target: &str, port: u16) {
    let _ = write!(out, "\t@{name} path /{name}*\n");
    let _ = write!(out, "\thandle @{name} {OPEN_BRACE_WITH_SPACE}\n");
    let _ = write!(out, "\t\treverse_proxy {target}:{port}\n");
    out.push_str("\t}\n\n");
}

/// One service's routed path, plus the object-storage buckets it declares.
pub struct CaddyService<'a> {
    pub id: ServiceId,
    pub host: &'a str,
    pub internal_port: u16,
    /// Bucket names, in `bucket_purposes()` order, for the purposes this service has.
    pub buckets: Vec<String>,
}

/// Builds the Caddyfile for the enabled services.
///
/// Two passes in [`HUB_SERVICE_ORDER`]: every service's own route first, then every
/// bucket of every service. Then the minio catch-all — note `path /minio/*`, with a slash
/// before the star, unlike the service routes.
pub fn build_caddyfile(
    services: &[CaddyService<'_>],
    minio_host: &str,
    minio_port: u16,
) -> String {
    let ordered = |f: &mut dyn FnMut(&CaddyService<'_>)| {
        for id in HUB_SERVICE_ORDER {
            if let Some(service) = services.iter().find(|s| s.id == id) {
                f(service);
            }
        }
    };

    let mut out = String::from("http:// {\n");

    ordered(&mut |service| route(&mut out, service.host, service.host, service.internal_port));
    ordered(&mut |service| {
        for bucket in &service.buckets {
            route(&mut out, bucket, minio_host, minio_port);
        }
    });

    // A hub serves no `/.well-known` of its own: clients resolve it against the
    // coordination server, which is where the JWKS lives.
    let _ = write!(out, "\t@minio path /minio/*\n");
    let _ = write!(out, "\thandle @minio {OPEN_BRACE_BARE}\n");
    let _ = write!(out, "\t\treverse_proxy {minio_host}:{minio_port}\n");
    out.push_str("\t}\n\n");

    out.push_str("}\n");
    out
}
