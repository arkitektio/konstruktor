//! What a service's image says of itself, and its config written by it.
//!
//! A hub's installer knows the hub: where the database is, which services run, which keys
//! they trust. What a *service* is — what it needs, how this release of it spells its
//! config — the service's own image says, through one entry point every image has
//! (`python -m hub_contract <verb>`, the `hub-contract` package):
//!
//! - `describe`: what it needs from a hub and offers to it ([`Description`]);
//! - `render`: this release's config, from the hub's facts ([`facts`]) with what the
//!   operator set laid over ([`crate::overrides`]).
//!
//! So a key a release renames is renamed in that release's image, and nothing here has to
//! learn of it. An image that does not answer `describe` has no contract: its config is
//! the one this installer has always generated for it ([`crate::generate::service`]).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_norway::Value;

use crate::catalog::{ServiceId, HOOKED_SERVICES};
use crate::config::hub::HubConfig;
use crate::generate::service::{build_service_config, map, s};
use crate::generate::IssuedIdentity;

/// The version of the contract this build speaks.
pub const CONTRACT: u32 = 1;

/// A release's own no: facts it cannot be configured from, an override it does not read.
const REFUSED: i32 = 78;

pub const FACTS_DIR: &str = "facts";

/// What a service needs a hub to provide.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Needs {
    #[serde(default)]
    pub storage: Vec<String>,
    #[serde(default)]
    pub instance_key: bool,
    #[serde(default)]
    pub peers: Vec<String>,
}

/// What a service offers a hub.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offers {
    #[serde(default)]
    pub health: String,
    /// Endpoints other services are wired to, by kind, as paths under the service's own.
    #[serde(default)]
    pub endpoints: BTreeMap<String, String>,
}

/// A service, as its image describes it (`hub_contract.description.Description`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Description {
    pub contract: u32,
    pub name: String,
    #[serde(default)]
    pub needs: Needs,
    #[serde(default)]
    pub offers: Offers,
    /// Peers this release only works beside in certain versions (`rekuest: ">=6"`).
    #[serde(default)]
    pub requires: BTreeMap<String, String>,
    /// The oldest version a deployment can be moved to this release from directly.
    #[serde(default)]
    pub upgrade_from: Option<String>,
}

/// Asks `image` to describe itself. `None` for an image with no contract — every release
/// before there was one — or one that speaks a contract this build does not.
pub async fn describe(image: &str) -> Option<Description> {
    let output = crate::engine_probe::engine()
        .async_command()
        .args([
            "run",
            "--rm",
            image,
            "python",
            "-m",
            "hub_contract",
            "describe",
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()
        .filter(|output| output.status.success())?;
    serde_json::from_slice::<Description>(&output.stdout)
        .ok()
        .filter(|said| said.contract == CONTRACT)
}

/// What an image answered when asked for its config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rendered {
    /// This release's config, as YAML.
    Config(String),
    /// Its own no, in its words: facts it cannot be configured from, an override it does
    /// not read.
    Refused(String),
    /// The asking went wrong.
    Failed(String),
}

fn rendered_from(code: Option<i32>, stdout: &str, stderr: &str) -> Rendered {
    match code {
        Some(0) => Rendered::Config(stdout.to_string()),
        Some(REFUSED) => Rendered::Refused(stderr.trim().to_string()),
        _ => Rendered::Failed(stderr.trim().to_string()),
    }
}

/// Has `image` write its config from the facts at `facts`, with `overrides` laid over if
/// that file exists. Both are mounted read-only; nothing else of the hub is.
pub async fn render(image: &str, facts: &Path, overrides: &Path) -> Rendered {
    let absolute = |path: &Path| {
        std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_string()
    };
    let mut args: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "-v".into(),
        format!("{}:/hub/facts.yaml:ro", absolute(facts)),
    ];
    if overrides.is_file() {
        args.push("-v".into());
        args.push(format!("{}:/hub/overrides.yaml:ro", absolute(overrides)));
    }
    args.extend(
        [image, "python", "-m", "hub_contract", "render"]
            .into_iter()
            .map(String::from),
    );
    let output = crate::engine_probe::engine()
        .async_command()
        .args(&args)
        .stdin(std::process::Stdio::null())
        .output()
        .await;
    match output {
        Ok(out) => rendered_from(
            out.status.code(),
            &String::from_utf8_lossy(&out.stdout),
            &String::from_utf8_lossy(&out.stderr),
        ),
        Err(error) => Rendered::Failed(error.to_string()),
    }
}

/// The paths a service of `id` offers when its image does not say: what this installer
/// knew of the services before any described itself.
fn known_endpoints(id: ServiceId) -> BTreeMap<String, String> {
    match HOOKED_SERVICES.contains(&id) {
        true => BTreeMap::from([
            (
                "rekuest_service".to_string(),
                "_rekuest/service".to_string(),
            ),
            ("rekuest_hook".to_string(), "_rekuest/hook".to_string()),
        ]),
        false => BTreeMap::new(),
    }
}

/// What the hub tells the service `id` about itself (`hub_contract.facts.Facts`): the one
/// input its config is written from.
///
/// Nothing here is in the service's vocabulary. It is read off what this installer already
/// works out for every service — its database, its buckets, its key, how tokens are
/// verified — and off the other services: where each is, and what each offers
/// (`described`, by compose service; for one that did not describe itself, what was
/// always known of it).
pub fn facts(
    config: &HubConfig,
    id: ServiceId,
    issued: &IssuedIdentity,
    described: &BTreeMap<String, Description>,
) -> Value {
    let service = config.service(id);
    // The blocks this installer generates are the hub's facts already, in one service's
    // spelling; read back out of it, they are in nobody's.
    let generated = build_service_config(config, id, issued);
    let block = |name: &str| generated.get(name).cloned();
    let field = |block: &Option<Value>, name: &str| {
        block
            .as_ref()
            .and_then(|block| block.get(name))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let url = |host: &str, port: u16| format!("http://{host}:{port}/{host}");

    let django = block("django");
    let mut me = vec![
        ("name", s(&service.host)),
        ("path", s(&service.host)),
        ("url", s(&url(&service.host, service.internal_port))),
        ("identifier", s(&format!("live.arkitekt.{}", id.as_str()))),
        ("secret_key", field(&django, "secret_key")),
        ("debug", field(&django, "debug")),
        ("allowed_hosts", field(&django, "hosts")),
        ("admin", field(&django, "admin")),
    ];
    if let Some(issuer) = &service.provenance_issuer {
        me.push(("settings", map(vec![("provenance_issuer", s(issuer))])));
    }
    let mut out = vec![
        ("facts", Value::Number(1.into())),
        ("me", map(me)),
        (
            "hub",
            map(vec![
                ("origins", field(&django, "csrf_trusted_origins")),
                ("auth", block("authentikate").unwrap_or(Value::Null)),
            ]),
        ),
    ];

    let postgres = block("postgres");
    out.push((
        "database",
        map(vec![
            ("host", field(&postgres, "host")),
            ("port", field(&postgres, "port")),
            ("name", field(&postgres, "db_name")),
            ("username", field(&postgres, "username")),
            ("password", field(&postgres, "password")),
        ]),
    ));
    if let Some(redis) = block("redis") {
        out.push(("redis", redis));
    }
    if let Some(Value::Mapping(datalayer)) = block("datalayer") {
        // A bucket is the one kind of entry that is a mapping: `media: {bucket: name}`.
        let mut storage = serde_norway::Mapping::new();
        let mut buckets = serde_norway::Mapping::new();
        for (key, value) in datalayer {
            match value.get("bucket").cloned() {
                Some(bucket) => buckets.insert(key, bucket),
                None => storage.insert(key, value),
            };
        }
        storage.insert("buckets".into(), Value::Mapping(buckets));
        out.push(("storage", Value::Mapping(storage)));
    }
    if let Some(instance) = block("instance") {
        out.push(("instance", instance));
    }
    if service.fernet_key.is_some() {
        out.push((
            "secrets",
            map(vec![(
                "fernet",
                s(&crate::generate::service::fernet_key_path(service)),
            )]),
        ));
    }

    // --- the rest of the hub ----------------------------------------------------------
    let mut peers: Vec<(String, Value)> = Vec::new();
    for other in config.enabled_services() {
        let peer = config.service(other);
        if other == id || peer.image.is_none() {
            continue;
        }
        let base = url(&peer.host, peer.internal_port);
        let endpoints = described
            .get(&peer.host)
            .map(|said| said.offers.endpoints.clone())
            .unwrap_or_else(|| known_endpoints(other));
        let mut offers: Vec<(String, Value)> = endpoints
            .iter()
            .map(|(kind, path)| (kind.clone(), s(&format!("{base}/{path}"))))
            .collect();
        // The agents' endpoint of the hub's Rekuest is takt's: what a hooked service
        // reports to.
        if other == ServiceId::Rekuest {
            if let Some(takt) = config.takt_url() {
                offers.push(("agent".to_string(), s(&takt)));
            }
        }
        peers.push((
            peer.host.clone(),
            map(vec![
                (
                    "identifier",
                    s(&format!("live.arkitekt.{}", other.as_str())),
                ),
                ("url", s(&base)),
                (
                    "offers",
                    Value::Mapping(offers.into_iter().map(|(k, v)| (k.into(), v)).collect()),
                ),
            ]),
        ));
    }
    // What runs beside the services and is no service of the hub: Rekuest's other half,
    // a model server, a media server.
    if id == ServiceId::Rekuest {
        if let Some(takt) = config.takt_url() {
            peers.push((
                "takt".to_string(),
                map(vec![
                    ("url", s(&takt)),
                    (
                        "settings",
                        map(vec![("socket", s(crate::config::hub::TAKT_SOCKET_PATH))]),
                    ),
                ]),
            ));
        }
    }
    if let Some(ollama) = &config.local_ollama {
        peers.push(("ollama".to_string(), map(vec![("url", s(&ollama.url))])));
    }
    if let Some(livekit) = config.running_livekit() {
        peers.push((
            "livekit".to_string(),
            map(vec![
                ("url", s(&livekit.api_url())),
                (
                    "settings",
                    map(vec![
                        ("api_key", s(&livekit.api_key)),
                        ("api_secret", s(&livekit.api_secret)),
                    ]),
                ),
            ]),
        ));
    }
    out.push((
        "peers",
        Value::Mapping(peers.into_iter().map(|(k, v)| (k.into(), v)).collect()),
    ));
    map(out)
}

/// Why a release that asks for certain versions beside it cannot run beside `versions`
/// (peer name to the version that will run), if it cannot.
pub fn unmet(said: &Description, versions: &BTreeMap<String, String>) -> Option<String> {
    said.requires.iter().find_map(|(peer, wanted)| {
        let runs = versions.get(peer)?;
        (!satisfies(runs, wanted)).then(|| {
            format!("it needs `{peer}` {wanted}, and this hub would run `{peer}` {runs} beside it")
        })
    })
}

fn numbers(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .map_while(|part| part.parse().ok())
        .collect()
}

/// Whether `version` is at least `oldest`, by its numbers (`6.1.0` ≥ `6`).
pub fn at_least(version: &str, oldest: &str) -> bool {
    numbers(version) >= numbers(oldest)
}

/// Whether `version` meets a requirement like `>=6`, `>=6,<8`, `==6.1`.
pub fn satisfies(version: &str, wanted: &str) -> bool {
    wanted.split(',').map(str::trim).all(|clause| {
        let found = numbers(version);
        let test = |operator: &str, check: &dyn Fn(&[u64], &[u64]) -> bool| {
            clause
                .strip_prefix(operator)
                .map(|bound| check(&found, &numbers(bound)))
        };
        test(">=", &|a, b| a >= b)
            .or_else(|| test("<=", &|a, b| a <= b))
            .or_else(|| test("==", &|a, b| a.starts_with(b)))
            .or_else(|| test(">", &|a, b| a > b))
            .or_else(|| test("<", &|a, b| a < b))
            // Something this build cannot read is not a reason to refuse an update.
            .unwrap_or(true)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::hub::{build_hub_config, HubConfigOptions};

    #[test]
    fn a_requirement_is_read_by_its_numbers() {
        assert!(satisfies("6.1.0", ">=6"));
        assert!(satisfies("6.0.0", ">=6,<7"));
        assert!(!satisfies("5.2.0", ">=6"));
        assert!(!satisfies("7.0.0", ">=6,<7"));
        assert!(satisfies("6.1.3", "==6.1"));
        assert!(satisfies("6.1.0-rc.1", ">=6.1"));
        assert!(satisfies("6.0.0", "whatever"));
        assert!(at_least("5.2.0", "5.0.0") && !at_least("4.9.0", "5.0.0"));

        let said = Description {
            requires: BTreeMap::from([("rekuest".to_string(), ">=6".to_string())]),
            ..Description::default()
        };
        let beside = |version: &str| BTreeMap::from([("rekuest".to_string(), version.to_string())]);
        assert!(unmet(&said, &beside("5.2.0")).unwrap().contains("rekuest"));
        assert_eq!(unmet(&said, &beside("6.0.0")), None);
        // A peer the hub does not run is not a version to object to.
        assert_eq!(unmet(&said, &BTreeMap::new()), None);
    }

    #[test]
    fn only_an_images_own_no_is_a_refusal() {
        assert_eq!(
            rendered_from(Some(0), "django: {}\n", ""),
            Rendered::Config("django: {}\n".into())
        );
        assert_eq!(
            rendered_from(
                Some(78),
                "",
                "mikro: this release does not read:\n  django.debgu\n"
            ),
            Rendered::Refused("mikro: this release does not read:\n  django.debgu".into())
        );
        assert!(matches!(
            rendered_from(Some(1), "", "Traceback"),
            Rendered::Failed(_)
        ));
    }

    /// The facts are the hub's, in nobody's spelling: Rekuest is told who offers what, and
    /// a hooked service where the agents' endpoint is.
    #[test]
    fn the_hub_tells_each_service_about_itself_and_the_others() {
        let config = build_hub_config(&HubConfigOptions::default());
        let described = BTreeMap::from([(
            "kraph".to_string(),
            Description {
                contract: 1,
                name: "kraph".into(),
                offers: Offers {
                    health: "ht".into(),
                    endpoints: BTreeMap::from([(
                        "rekuest_hook".to_string(),
                        "_hooks/rekuest".to_string(),
                    )]),
                },
                ..Description::default()
            },
        )]);

        let rekuest = facts(&config, ServiceId::Rekuest, &Default::default(), &described);
        assert_eq!(
            rekuest["me"]["url"].as_str(),
            Some("http://rekuest:80/rekuest")
        );
        assert_eq!(rekuest["database"]["name"].as_str(), Some("rekuest"));
        assert_eq!(
            rekuest["storage"]["buckets"]["media"].as_str(),
            Some("rekuestmedia")
        );
        assert!(rekuest["storage"].get("media").is_none());
        assert!(rekuest["instance"]["private_key"].as_str().is_some());
        assert_eq!(
            rekuest["peers"]["takt"]["settings"]["socket"].as_str(),
            Some("/run/takt/internal.sock")
        );
        // What a service said it offers, where it said; what was always known of the rest.
        assert_eq!(
            rekuest["peers"]["kraph"]["offers"]["rekuest_hook"].as_str(),
            Some("http://kraph:80/kraph/_hooks/rekuest")
        );
        assert_eq!(
            rekuest["peers"]["mikro"]["offers"]["rekuest_service"].as_str(),
            Some("http://mikro:80/mikro/_rekuest/service")
        );
        assert!(rekuest["peers"].get("rekuest").is_none());

        let mikro = facts(&config, ServiceId::Mikro, &Default::default(), &described);
        assert_eq!(
            mikro["peers"]["rekuest"]["offers"]["agent"].as_str(),
            Some("http://rekuest-takt:8080/rekuest")
        );
        assert!(mikro["peers"].get("takt").is_none());
        assert_eq!(
            mikro["storage"]["buckets"]["zarr"].as_str(),
            Some("mikrozarr")
        );
    }
}
