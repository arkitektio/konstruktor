use serde_norway::{Mapping, Value};

use crate::catalog::ServiceId;
use crate::config::hub::{HubConfig, ServiceBlock};
use crate::generate::IssuedIdentity;
use crate::secrets::public_jwk;

/// Small helpers for building YAML documents by hand. The generated shapes are
/// heterogeneous enough that modelling every one as a struct would cost more than it
/// buys — these keep the construction readable.
pub(crate) fn map(pairs: Vec<(&str, Value)>) -> Value {
    let mut m = Mapping::new();
    for (k, v) in pairs {
        m.insert(Value::from(k), v);
    }
    Value::Mapping(m)
}

pub(crate) fn s(value: &str) -> Value {
    Value::from(value)
}

pub(crate) fn list(values: Vec<Value>) -> Value {
    Value::Sequence(values)
}

/// The `aud` authentikate will accept: anything. See [`build_authentikate`].
const ANY_AUDIENCE: &str = "*";

/// An issuer whose keys are written into the config itself — authentikate's `jwks_dict`.
fn inline_issuer(iss: &str, jwks: Value) -> Value {
    map(vec![
        ("iss", s(iss)),
        ("jwks", jwks),
        ("kind", s("jwks_dict")),
    ])
}

/// `{"keys": [...]}`, as YAML.
fn jwks_of(keys: Vec<serde_json::Value>) -> Value {
    serde_norway::to_value(serde_json::json!({ "keys": keys })).expect("a JWKS is plain data")
}

/// The hub's trust bundle, inline: every enabled service instance's public key, under its
/// service. What the coordination server serves as the hub's `hub_keys_url`, for a hub that
/// was not handed one (not enrolled yet) — without it, every signed request between the
/// hub's services fails with "No key … in the hub's trust bundle". Same shape as
/// `scripts/instance_keys.py` in the reference deployment writes.
///
/// A key is listed under what its service is registered as, which is the image's to say
/// ([`ServiceBlock::identifier`]): a service that holds a key and was never asked what it
/// is has nothing to be listed under, and is left out.
fn inline_trust_bundle(config: &HubConfig) -> Option<Value> {
    let keys: Vec<serde_json::Value> = config
        .enabled_services()
        .into_iter()
        .filter_map(|id| {
            let block = config.service(id);
            let pair = block.instance_key_pair.as_ref()?;
            public_jwk(pair, block.identifier.as_deref()?)
        })
        .collect();
    (!keys.is_empty()).then(|| jwks_of(keys))
}

fn jwks_issuer(iss: &str, jwks_uri: &str) -> Value {
    map(vec![
        ("iss", s(iss)),
        ("jwks_uri", s(jwks_uri)),
        ("kind", s("jwks_uri")),
    ])
}

/// The coordination server's verification keys, where they are actually served.
///
/// Older grants handed back `https://<server>/lok/.well-known/jwks.json`, which is a
/// 404: that well-known is served from the server's root, not from behind the `/lok/`
/// prefix. A service that cannot fetch the key set rejects every token it is given, so
/// that one shape — a well-known behind a prefix — is moved to the root.
///
/// Anything else is taken as granted. Current servers hand back Lok's own key route,
/// `https://<server>/lok/o/jwks/`, which is served where it says and anchored to the
/// issuer; rewriting it to the root well-known only kept working because that route is
/// kept for older hubs.
fn jwks_at_base(url: &str) -> String {
    let (scheme, rest) = match url.split_once("://") {
        Some((scheme, rest)) => (scheme, rest),
        // Not a URL we can take apart — a bare host, most likely. Left alone but for the
        // scheme, since guessing at its shape would be worse than passing it through.
        None => return format!("https://{}/{JWKS_PATH}", url.trim_matches('/')),
    };
    if !rest.ends_with(JWKS_PATH) {
        return url.to_string();
    }
    let authority = rest.split('/').next().unwrap_or(rest);
    format!("{scheme}://{authority}/{JWKS_PATH}")
}

const JWKS_PATH: &str = ".well-known/jwks.json";

/// Where a service finds one of its secrets inside its container. See
/// [`crate::generate::compose`], which mounts `secrets/<host>.<name>` there.
pub fn secret_path(service: &ServiceBlock, name: &str) -> String {
    format!("/secrets/{}.{name}", service.host)
}

/// The file in the deployment folder holding one of a service's secrets.
pub fn secret_file(service: &ServiceBlock, name: &str) -> String {
    format!("secrets/{}.{name}", service.host)
}

/// Inbound token verification.
///
/// The issuer is the remote coordination server — unless the hub runs its own, in which
/// case it is that Lok's signing key, inline (see [`crate::generate::lok::rsa_issuer`]) and
/// whatever a grant said is beside the point: there was none. Provenance points at the
/// local Rekuest when it runs here, and at the configured remote one otherwise.
///
/// The issuer string comes from the grant and is used verbatim — authentikate selects a
/// trust anchor by exact string equality, and `https://<host>` is not `<host>`. The JWKS
/// URL is the one place the grant is not taken at its word: see [`jwks_at_base`].
pub fn build_authentikate(config: &HubConfig, issued: &IssuedIdentity) -> Value {
    let issuer = match config.running_lok() {
        Some(lok) => crate::generate::lok::rsa_issuer(lok),
        None => {
            let iss = issued
                .issuer
                .clone()
                .unwrap_or_else(|| config.coord_server.clone());
            let jwks = issued
                .jwks_url
                .as_deref()
                .map(jwks_at_base)
                .unwrap_or_else(|| format!("https://{}/{JWKS_PATH}", config.coord_server));
            jwks_issuer(&iss, &jwks)
        }
    };

    let mut pairs = vec![
        // Every audience, for now. Authentikate began requiring `aud` to be declared —
        // a config without it does not start — and nothing in the grant tells us which
        // audience a token issued for this hub will carry. `*` accepts what the services
        // already accepted before the field existed, so it is the honest translation of
        // "not checked" rather than a guess that would silently reject every token.
        //
        // To be replaced by the service instance the configure request returns, once it
        // returns one.
        ("audience", s(ANY_AUDIENCE)),
        ("issuers", list(vec![issuer])),
        ("static_tokens", Value::Mapping(Mapping::new())),
    ];

    let rekuest = config.rekuest();
    let remote = config.rekuest_server.trim();

    let provenance = if rekuest.enabled {
        let iss = rekuest.provenance_issuer.as_deref().unwrap_or_default();
        // Rekuest signs provenance with its instance key. The coordination server's trust
        // bundle, narrowed to Rekuest, is the vouched-for source. Without one (a hub not
        // enrolled yet), the key itself, inline — Konstruktor minted it, so it needs no
        // fetch. Both name Rekuest by what it is registered as; a Rekuest whose image was
        // never asked, or whose key cannot be read, falls back to its own key set inside
        // the network, behind its script name.
        let registered = rekuest.identifier.as_deref();
        match (issued.hub_keys_url.as_deref(), registered) {
            (Some(url), Some(identifier)) => {
                Some(jwks_issuer(iss, &format!("{url}?service={identifier}")))
            }
            _ => Some(
                rekuest
                    .instance_key_pair
                    .as_ref()
                    .zip(registered)
                    .and_then(|(pair, identifier)| public_jwk(pair, identifier))
                    .map(|jwk| inline_issuer(iss, jwks_of(vec![jwk])))
                    .unwrap_or_else(|| {
                        jwks_issuer(
                            iss,
                            &format!(
                                "http://{}:{}/{}/{JWKS_PATH}",
                                rekuest.host, rekuest.internal_port, rekuest.host
                            ),
                        )
                    }),
            ),
        }
    } else if !matches!(remote, "local" | "none" | "") {
        Some(jwks_issuer(
            remote,
            &format!("https://{remote}/{JWKS_PATH}"),
        ))
    } else {
        None
    };

    if let Some(issuer) = provenance {
        pairs.push((
            "provenance",
            map(vec![
                ("audience", s(ANY_AUDIENCE)),
                ("issuers", list(vec![issuer])),
            ]),
        ));
    }

    map(pairs)
}

/// The role the datalayer assumes for its scoped sessions. See [`build_datalayer`].
pub const DATALAYER_ROLE_ARN: &str = "arn:aws:iam::000000000000:role/datalayer";

fn build_datalayer(config: &HubConfig, service: &ServiceBlock) -> Value {
    let mut pairs = vec![
        // Every upload and download grant is an STS session scoped by an inline policy,
        // and the services refuse to issue one without a role to assume — falling back to
        // their own unscoped key only if told to, which is for development. RustFS, like
        // MinIO, ignores the ARN itself; any ARN-shaped string does, and this is the one
        // the services' own example configs use.
        ("role_arn", s(DATALAYER_ROLE_ARN)),
        ("session_duration_seconds", Value::from(3600)),
        ("access_key", s(&config.minio.access_key)),
        ("secret_key", s(&config.minio.secret_key)),
        ("host", s(&config.minio.host)),
        ("port", Value::from(config.minio.internal_port)),
        ("protocol", s("http")),
        ("region", s("us-east-1")),
    ];

    for (purpose, name) in service.buckets.iter() {
        pairs.push((purpose, map(vec![("bucket", s(name))])));
    }

    map(pairs)
}

/// What the hub provides one service, block by block: its Django settings, its database,
/// the redis, how tokens are verified, its object storage and buckets, its key and the
/// trust bundle.
///
/// This is not a service's config. A service's config is its own image's to write
/// ([`crate::contract`]); these are the hub's facts for it, kept in the shape they were
/// first written in, and read back out of it into a document in nobody's vocabulary.
pub(crate) fn hub_blocks(config: &HubConfig, id: ServiceId, issued: &IssuedIdentity) -> Value {
    let service = config.service(id);

    let csrf = config
        .csrf_trusted_origins
        .clone()
        .unwrap_or_else(|| vec!["http://localhost".into(), "https://localhost".into()]);

    let mut django = Vec::new();
    // Only a service that asked to be told of the operator account is.
    if service.admin_config.is_some() {
        django.push((
            "admin",
            map(vec![
                (
                    "email",
                    config
                        .global_admin_email
                        .as_deref()
                        .map(s)
                        .unwrap_or(Value::Null),
                ),
                ("password", s(&config.global_admin_password)),
                ("username", s(&config.global_admin)),
            ]),
        ));
    }
    django.extend([
        (
            "csrf_trusted_origins",
            list(csrf.iter().map(|o| s(o)).collect()),
        ),
        ("debug", Value::from(service.debug)),
        // No leading slash: services append this to an absolute base when
        // building external URLs, and a slash here would produce `//lok/o/token/`.
        ("force_script_name", s(&service.host)),
        (
            "hosts",
            list(service.allowed_hosts.iter().map(|h| s(h)).collect()),
        ),
        ("secret_key", s(&service.secret_key)),
    ]);

    let mut pairs = vec![("django", map(django))];
    // The database and the Redis, for a service that declared it uses them.
    if let Some(database) = service.database() {
        pairs.push((
            "postgres",
            map(vec![
                ("db_name", s(database)),
                ("engine", s("django.db.backends.postgresql")),
                ("host", s("db")),
                ("password", s(&config.db.postgres_password)),
                ("port", Value::from(5432)),
                ("username", s(&config.db.postgres_user)),
            ]),
        ));
    }
    if service.redis_config.is_some() {
        pairs.push((
            "redis",
            map(vec![
                ("host", s(&config.local_redis.host)),
                ("port", Value::from(config.local_redis.internal_port)),
            ]),
        ));
    }
    pairs.push(("authentikate", build_authentikate(config, issued)));

    // The object store, with its credentials, exactly for a service that declared storage.
    if service.uses_datalayer() {
        pairs.push(("datalayer", build_datalayer(config, service)));
    }

    // This instance's key — its only secret towards the hub's other services — and where the
    // keys live it trusts (the coordination server's bundle of this hub's instance keys).
    if let Some(pair) = &service.instance_key_pair {
        let mut instance = vec![("private_key", s(&pair.private_key))];
        match issued.hub_keys_url.as_deref() {
            Some(url) => instance.push(("trust", map(vec![("jwks_uri", s(url))]))),
            None => {
                if let Some(bundle) = inline_trust_bundle(config) {
                    instance.push(("trust", map(vec![("jwks", bundle)])));
                }
            }
        }
        pairs.push(("instance", map(instance)));
    }

    map(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point: a grant that still names Lok's mount point must not be written
    /// into a config, because nothing is served there.
    #[test]
    fn the_key_set_is_taken_from_the_base_route() {
        assert_eq!(
            jwks_at_base("https://go.arkitekt.live/lok/.well-known/jwks.json"),
            "https://go.arkitekt.live/.well-known/jwks.json"
        );
        // Already right, and left exactly as it is.
        assert_eq!(
            jwks_at_base("https://go.arkitekt.live/.well-known/jwks.json"),
            "https://go.arkitekt.live/.well-known/jwks.json"
        );
        // A port survives; it is part of the authority.
        assert_eq!(
            jwks_at_base("http://localhost:8000/lok/.well-known/jwks.json"),
            "http://localhost:8000/.well-known/jwks.json"
        );
        // Not a URL at all — the host is kept and https assumed.
        assert_eq!(
            jwks_at_base("coord.example.org"),
            "https://coord.example.org/.well-known/jwks.json"
        );
        // Lok's own key route, which current servers grant, is served where it says.
        assert_eq!(
            jwks_at_base("https://go.arkitekt.live/lok/o/jwks/"),
            "https://go.arkitekt.live/lok/o/jwks/"
        );
    }
}
