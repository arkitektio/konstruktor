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
pub fn build_caddyfile(services: &[CaddyService<'_>], minio_host: &str, minio_port: u16) -> String {
    let ordered = |f: &mut dyn FnMut(&CaddyService<'_>)| {
        for id in HUB_SERVICE_ORDER {
            if let Some(service) = services.iter().find(|s| s.id == id) {
                f(service);
            }
        }
    };

    let mut out = String::from("http:// {\n");
    out.push_str(CORS);

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

    // The object store's own endpoints, under a prefix that is stripped on the way in —
    // which is what makes `/rustfs/health` the store's `/health`. That is the challenge
    // the `live.arkitekt.s3` instance advertises, as upstream's generator does.
    let _ = write!(out, "\t@rustfs path {RUSTFS_PREFIX}/*\n");
    let _ = write!(out, "\thandle @rustfs {OPEN_BRACE_BARE}\n");
    let _ = write!(out, "\t\turi strip_prefix {RUSTFS_PREFIX}\n");
    let _ = write!(out, "\t\treverse_proxy {minio_host}:{minio_port}\n");
    out.push_str("\t}\n\n");

    out.push_str("}\n");
    out
}

/// The response headers every route gets, and the preflight answer — the same set the
/// Arkitekt lab gateway serves.
///
/// Browser and Electron clients reach every alias cross-origin — fakts checks each one
/// with a `fetch` — and the object store answers no CORS at all, so its health check,
/// every presigned upload and every ranged zarr read were discarded by the browser
/// however well they went.
///
/// * The origin is echoed rather than `*`.
/// * The allowed headers are listed, not `*`: a wildcard does not cover `Authorization`,
///   and `Range` is there because ranged zarr reads preflight on it.
/// * S3's headers are exposed so zarr clients can read ETags, lengths and ranges.
/// * The CORS fields are set with `>`, deferred until the response is written, so they
///   replace what a Django service sends instead of adding a second value — two
///   `Access-Control-Allow-Origin` values are rejected just like none.
/// * Preflights are answered here, in the first `handle`, and reach no service: named
///   `handle` blocks run in the order they are written.
const CORS: &str = "\theader {\n\
\t\t-Server\n\
\t\tX-Forwarded-Proto {scheme}\n\
\t\tX-Forwarded-For {remote}\n\
\t\tX-Forwarded-Port {server_port}\n\
\t\tX-Forwarded-Host {host}\n\
\t\t>Access-Control-Allow-Origin \"{header.Origin}\"\n\
\t\t>Access-Control-Allow-Methods \"GET, POST, PUT, PATCH, DELETE, OPTIONS\"\n\
\t\t>Access-Control-Allow-Headers \"Origin, Content-Type, Accept, Authorization, X-Requested-With, Range, x-host-override, x-amz-content-sha256, x-amz-date, x-amz-user-agent, x-amz-request-id, x-amz-security-token, x-amz-acl\"\n\
\t\t>Access-Control-Expose-Headers \"ETag, Content-Length, Content-Range, Accept-Ranges, x-amz-meta-dir, x-amz-request-id, x-amz-id-2\"\n\
\t\t>Access-Control-Max-Age \"86400\"\n\
\t\tTiming-Allow-Origin \"*\"\n\
\t}\n\n\
\t@options {\n\
\t\tmethod OPTIONS\n\
\t}\n\
\thandle @options {\n\
\t\trespond 204\n\
\t}\n\n";

/// Where the gateway exposes the object store's own endpoints.
pub const RUSTFS_PREFIX: &str = "/rustfs";
/// The object store's health check, through the gateway, relative to its root.
pub const S3_CHALLENGE: &str = "rustfs/health";
