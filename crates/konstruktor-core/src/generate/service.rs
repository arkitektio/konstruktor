use serde_norway::{Mapping, Value};

use crate::catalog::ServiceId;
use crate::config::hub::{HubConfig, ServiceBlock};
use crate::generate::IssuedIdentity;

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

fn jwks_issuer(iss: &str, jwks_uri: &str) -> Value {
    map(vec![
        ("iss", s(iss)),
        ("jwks_uri", s(jwks_uri)),
        ("kind", s("jwks_uri")),
    ])
}

/// The coordination server's verification keys, at the base route.
///
/// The keys are served from the server's own root — `https://<server>/.well-known/
/// jwks.json` — not from behind the `/lok/` prefix Lok is mounted under. A grant that
/// still hands back the `/lok/` URL is therefore pointing at a 404, and a service that
/// cannot fetch the key set rejects every token it is given, so the authority is taken
/// from the granted URL and the path is replaced rather than trusted.
fn jwks_at_base(url: &str) -> String {
    let (scheme, rest) = match url.split_once("://") {
        Some((scheme, rest)) => (scheme, rest),
        // Not a URL we can take apart — a bare host, most likely. Left alone but for the
        // scheme, since guessing at its shape would be worse than passing it through.
        None => return format!("https://{}/{JWKS_PATH}", url.trim_matches('/')),
    };
    let authority = rest.split('/').next().unwrap_or(rest);
    format!("{scheme}://{authority}/{JWKS_PATH}")
}

const JWKS_PATH: &str = ".well-known/jwks.json";

/// Inbound token verification.
///
/// A hub never runs Lok, so the issuer is always the remote coordination server;
/// provenance points at the local Rekuest when it runs here, and at the configured remote
/// one otherwise.
///
/// The issuer string comes from the grant and is used verbatim — authentikate selects a
/// trust anchor by exact string equality, and `https://<host>` is not `<host>`. The JWKS
/// URL is the one place the grant is not taken at its word: see [`jwks_at_base`].
pub fn build_authentikate(config: &HubConfig, issued: &IssuedIdentity) -> Value {
    let iss = issued
        .issuer
        .clone()
        .unwrap_or_else(|| config.coord_server.clone());
    let jwks = issued
        .jwks_url
        .as_deref()
        .map(jwks_at_base)
        .unwrap_or_else(|| format!("https://{}/{JWKS_PATH}", config.coord_server));

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
        ("issuers", list(vec![jwks_issuer(&iss, &jwks)])),
        ("static_tokens", Value::Mapping(Mapping::new())),
    ];

    let rekuest = &config.rekuest;
    let remote = config.rekuest_server.trim();

    let provenance = if rekuest.enabled {
        Some(jwks_issuer(
            rekuest.provenance_issuer.as_deref().unwrap_or_default(),
            // Rekuest's own key set, inside the network, behind its script name — that
            // prefix is the service's own URL space, not Lok's.
            &format!(
                "http://{}:{}/{}/{JWKS_PATH}",
                rekuest.host, rekuest.internal_port, rekuest.host
            ),
        ))
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

fn build_datalayer(config: &HubConfig, id: ServiceId, service: &ServiceBlock) -> Value {
    let mut pairs = vec![
        ("access_key", s(&config.minio.access_key)),
        ("secret_key", s(&config.minio.secret_key)),
        ("host", s(&config.minio.host)),
        ("port", Value::from(config.minio.internal_port)),
        ("protocol", s("http")),
        ("region", s("us-east-1")),
    ];

    // Kept as owned strings so the borrows outlive the loop.
    let buckets: Vec<(&str, String)> = id
        .bucket_purposes()
        .iter()
        .filter_map(|purpose| {
            service
                .bucket(purpose)
                .map(|b| (*purpose, b.bucket_name.clone()))
        })
        .collect();

    for (purpose, name) in &buckets {
        pairs.push((purpose, map(vec![("bucket", s(name))])));
    }

    map(pairs)
}

/// The `configs/<service>.yaml` a service reads at startup.
pub fn build_service_config(config: &HubConfig, id: ServiceId, issued: &IssuedIdentity) -> Value {
    let service = config.service(id);

    let csrf = config
        .csrf_trusted_origins
        .clone()
        .unwrap_or_else(|| vec!["http://localhost".into(), "https://localhost".into()]);

    let mut pairs = vec![
        (
            "django",
            map(vec![
                (
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
                ),
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
            ]),
        ),
        (
            "postgres",
            map(vec![
                ("db_name", s(&service.db_config.db)),
                ("engine", s("django.db.backends.postgresql")),
                ("host", s("db")),
                ("password", s(&config.db.postgres_password)),
                ("port", Value::from(5432)),
                ("username", s(&config.db.postgres_user)),
            ]),
        ),
        (
            "redis",
            map(vec![
                ("host", s(&config.local_redis.host)),
                ("port", Value::from(config.local_redis.internal_port)),
            ]),
        ),
        ("authentikate", build_authentikate(config, issued)),
    ];

    // A service that stores no objects itself still gets its buckets created — it just
    // receives no `datalayer` block, and upstream's models reject one.
    if id.uses_datalayer() {
        pairs.push(("datalayer", build_datalayer(config, id, service)));
    }

    if let Some(pair) = &service.provenance_key_pair {
        pairs.push((
            "provenance",
            map(vec![
                (
                    "issuer",
                    service
                        .provenance_issuer
                        .as_deref()
                        .map(s)
                        .unwrap_or(Value::Null),
                ),
                (
                    "kid",
                    service
                        .provenance_kid
                        .as_deref()
                        .map(s)
                        .unwrap_or(Value::Null),
                ),
                ("private_key", s(&pair.private_key)),
                ("public_key", s(&pair.public_key)),
            ]),
        ));
    }

    // --- beyond upstream ------------------------------------------------------
    //
    // The two blocks below have no counterpart in the Python generator, which writes
    // `ollama_config` and `ensured_repositories` into the *profile* and then emits
    // neither into the service's own config — so nothing ever reaches the container.
    //
    // **The key shapes here are inferred, not sourced.** If a service disagrees with
    // them, this is the place to correct, and the golden fixtures will not catch it:
    // both are emitted only when somebody asked for something upstream cannot express,
    // so a stock hub still generates exactly what the Python CLI generates.
    if id == ServiceId::Alpaka {
        if let Some(ollama) = &config.local_ollama {
            pairs.push(("ollama", map(vec![("url", s(&ollama.url))])));
        }
    }

    if id == ServiceId::Kabinet {
        if let Some(repositories) = service
            .ensured_repositories
            .as_ref()
            .filter(|asked| !is_the_seeded_default(asked))
        {
            pairs.push((
                "ensured_repositories",
                list(repositories.iter().map(|r| s(r)).collect()),
            ));
        }
    }

    map(pairs)
}

/// Whether Kabinet's repository list is the one the config builder seeds.
///
/// A hub nobody customized has to keep generating what upstream generates, and upstream
/// emits no such key at all — so the default is written into the profile (where upstream
/// puts it too) and left out of the generated config.
fn is_the_seeded_default(repositories: &[String]) -> bool {
    repositories == ["jhnnsrs/ome:main", "jhnnsrs/renderer:main"]
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
    }
}
