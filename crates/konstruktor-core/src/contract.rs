//! What a service's image says of itself, and its config written by it.
//!
//! A hub's installer knows the hub: where the database is, which services run, which keys
//! they trust. What a *service* is — what it needs, how this release of it spells its
//! config — the service's own image says, through one entry point every image has
//! (`python -m arkitekt_service <verb>`, the `arkitekt-service` package):
//!
//! - `describe`: what it needs from a hub and offers to it ([`Description`]);
//! - `render`: this release's config, from the hub's facts ([`facts`]) with what the
//!   operator set laid over ([`crate::overrides`]).
//!
//! So a key a release renames is renamed in that release's image, and nothing here has to
//! learn of it. An image that does not answer `describe` has no contract, and is not a
//! release this installer runs.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_norway::Value;

use crate::catalog::ServiceId;
use crate::config::hub::HubConfig;
use crate::generate::service::{hub_blocks, map, s};
use crate::generate::IssuedIdentity;

/// The version of the contract this build speaks.
pub const CONTRACT: u32 = 1;

/// A release's own no: facts it cannot be configured from, an override it does not read.
const REFUSED: i32 = 78;

pub const FACTS_DIR: &str = "facts";

/// A permission or a role a service declares, with what it means.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub key: String,
    #[serde(default)]
    pub description: String,
}

/// What a service needs a hub to provide.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Needs {
    #[serde(default)]
    pub storage: Vec<String>,
    /// What a token may be allowed to do at the service, defined at the coordination
    /// server when the hub enrols.
    #[serde(default)]
    pub scopes: Vec<Scope>,
    /// The roles a member of an organization can hold at the service.
    #[serde(default)]
    pub roles: Vec<Scope>,
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

/// A service, as its image describes it (`arkitekt_service.contract.description.Description`).
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
            "arkitekt_service",
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
        [image, "python", "-m", "arkitekt_service", "render"]
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

/// A service whose image would not write its config.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// The release's own no, in its words.
    #[error("`{service}` cannot be configured for this hub as it is. {said}")]
    Refused { service: String, said: String },
    #[error("`{service}`'s image failed to write its config: {said}")]
    Failed { service: String, said: String },
    /// The image does not answer the contract at all: no release this installer can run.
    #[error(
        "`{service}` runs {image}, which does not answer the hub contract (`python -m \
         arkitekt_service describe`). A service's config is its own image's to write, so \
         this Konstruktor runs only releases that do: update `{service}` to one."
    )]
    NoContract { service: String, image: String },
    #[error("{0}")]
    Write(String),
}

/// The image each enabled service runs: the build written down for it, or what the profile
/// names.
fn images(dir: &Path, config: &HubConfig) -> Vec<(ServiceId, String, String)> {
    let pinned = crate::pins::references(config, &crate::lock::read(dir).pins);
    config
        .enabled_services()
        .into_iter()
        .filter_map(|id| {
            let block = config.service(id);
            let image = pinned
                .get(&block.host)
                .cloned()
                .or_else(|| block.image.clone())?;
            Some((id, block.host.clone(), image))
        })
        .collect()
}

/// What every enabled service's image says of itself, by compose service — asked once per
/// build and remembered in the lock. A service without a contract is not in the answer.
pub async fn described(dir: &Path, config: &HubConfig) -> BTreeMap<String, Description> {
    let mut held = crate::lock::read(dir);
    let mut asked = false;
    let mut out = BTreeMap::new();
    for (_, host, image) in images(dir, config) {
        let known = held
            .described
            .get(&host)
            .filter(|known| known.image == image)
            .cloned();
        let said = match known {
            Some(known) => known.description,
            None => {
                let description = describe(&image).await;
                held.described.insert(
                    host.clone(),
                    crate::lock::Described {
                        image,
                        description: description.clone(),
                    },
                );
                asked = true;
                description
            }
        };
        if let Some(said) = said {
            out.insert(host, said);
        }
    }
    if asked {
        // Re-read: describing takes a while, and the lock may have been written meanwhile.
        let mut now = crate::lock::read(dir);
        now.described = held.described;
        let _ = crate::lock::write(dir, &now);
    }
    out
}

/// Has every service's image write its own config.
///
/// For each: the hub's facts go into `facts/<service>.yaml` — the one file about a service
/// this installer writes of its own knowledge — and the image turns them, with the
/// operator's overrides, into `configs/<service>.yaml`. An image is only asked again when
/// what it was asked with changed or the file is no longer what it wrote. Returns the
/// services whose config was written by their image, asked now or before.
pub async fn render_hub(
    dir: &Path,
    config: &HubConfig,
    issued: &IssuedIdentity,
) -> Result<Vec<String>, RenderError> {
    let said = described(dir, config).await;
    let written = |error: std::io::Error| RenderError::Write(error.to_string());
    let mut rendered = Vec::new();
    for (id, host, image) in images(dir, config) {
        if !said.contains_key(&host) {
            return Err(RenderError::NoContract {
                service: host,
                image,
            });
        }
        let document = crate::generate::dump(&facts(config, id, issued, &said));
        let overrides = crate::overrides::path(dir, &host);
        let from = crate::lock::digest(
            format!(
                "{image}\n{document}\n{}",
                std::fs::read_to_string(&overrides).unwrap_or_default()
            )
            .as_bytes(),
        );
        let target = dir.join(format!("configs/{host}.yaml"));
        let on_disk = std::fs::read(&target)
            .map(|bytes| crate::lock::digest(&bytes))
            .ok();
        let current = crate::lock::read(dir)
            .rendered
            .get(&host)
            .is_some_and(|mark| mark.from == from && Some(&mark.config) == on_disk.as_ref());
        rendered.push(host.clone());
        if current {
            continue;
        }

        let facts_file = dir.join(FACTS_DIR).join(format!("{host}.yaml"));
        std::fs::create_dir_all(dir.join(FACTS_DIR)).map_err(written)?;
        std::fs::write(&facts_file, &document).map_err(written)?;
        // It holds the service's secrets: its owner's alone, where the platform can say so.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&facts_file, std::fs::Permissions::from_mode(0o600));
        }
        let text = match render(&image, &facts_file, &overrides).await {
            Rendered::Config(text) => text,
            Rendered::Refused(said) => {
                return Err(RenderError::Refused {
                    service: host,
                    said,
                })
            }
            Rendered::Failed(said) => {
                return Err(RenderError::Failed {
                    service: host,
                    said,
                })
            }
        };
        std::fs::write(&target, &text).map_err(written)?;
        let mut held = crate::lock::read(dir);
        let config_digest = crate::lock::digest(text.as_bytes());
        held.files
            .insert(format!("configs/{host}.yaml"), config_digest.clone());
        held.rendered.insert(
            host,
            crate::lock::Rendering {
                from,
                config: config_digest,
            },
        );
        crate::lock::write(dir, &held).map_err(written)?;
    }
    Ok(rendered)
}

/// The command that brings `service`'s database to the build it is about to run: its
/// image's `migrate`, which waits for the database, applies the migrations and runs the
/// service's own setup — in a container of that build, beside nothing else of the service.
pub fn prepare(service: &str) -> Vec<String> {
    [
        "compose",
        "run",
        "--rm",
        "--no-deps",
        "-T",
        service,
        "python",
        "-m",
        "arkitekt_service",
        "migrate",
    ]
    .map(String::from)
    .to_vec()
}

/// The id of the image a reference resolves to on this machine.
async fn image_id(image: &str) -> Option<String> {
    crate::docker::image_states(&[(String::new(), image.to_string())])
        .await
        .ok()?
        .into_iter()
        .next()?
        .image_id
}

/// The services whose database is not prepared for the build they run, with that build's
/// id: never started on it, or started last on another.
pub async fn unprepared(dir: &Path, config: &HubConfig) -> Vec<(String, String)> {
    let prepared = crate::lock::read(dir).prepared;
    let mut out = Vec::new();
    for (_, host, image) in images(dir, config) {
        // An image that is not on the machine has no build to compare: it is prepared for
        // once it is.
        let Some(id) = image_id(&image).await else {
            continue;
        };
        if prepared.get(&host) != Some(&id) {
            out.push((host, id));
        }
    }
    out
}

/// Writes down that `service`'s database is prepared for the build with this id.
pub fn prepared(dir: &Path, service: &str, id: &str) -> std::io::Result<()> {
    let mut held = crate::lock::read(dir);
    held.prepared.insert(service.to_string(), id.to_string());
    crate::lock::write(dir, &held)
}

/// [`prepared`], for the build `service` runs now.
pub async fn prepared_for_its_build(dir: &Path, config: &HubConfig, service: &str) {
    for (_, host, image) in images(dir, config) {
        if host == service {
            if let Some(id) = image_id(&image).await {
                let _ = prepared(dir, service, &id);
            }
        }
    }
}

/// Prepares every service's database for the build it is about to be started on — once.
///
/// A service's own start only serves. What its database needs first is its image's to do
/// (`migrate`: wait, migrate, the service's setup), and this is when it is asked to: for a
/// build the database was not prepared for yet — a hub's first start, a service just
/// added, a build put back. A container that merely restarts, and a hub brought up again
/// on the builds it ran, cost nothing. The database is started for it if it is not up.
pub async fn prepare_databases(
    dir: &Path,
    config: &HubConfig,
    on_line: &(dyn Fn(crate::compose::ComposeLine) + Send + Sync),
) -> Result<Vec<String>, String> {
    let waiting = unprepared(dir, config).await;
    if waiting.is_empty() {
        return Ok(Vec::new());
    }
    let say = |line: String| on_line(crate::compose::ComposeLine { line, stderr: true });
    let up = vec![
        "compose".to_string(),
        "up".to_string(),
        "-d".to_string(),
        crate::config::hub::DB_COMPOSE_SERVICE.to_string(),
    ];
    crate::compose::run_streamed(dir, up, on_line).await?;
    crate::backup::wait_for_database(dir, config, "database", &|_| {})
        .await
        .map_err(|error| error.to_string())?;
    let mut done = Vec::new();
    for (service, id) in waiting {
        say(format!(
            "Preparing {service}'s database for the build it runs"
        ));
        crate::compose::run_streamed(dir, prepare(&service), on_line)
            .await
            .map_err(|said| {
                format!(
                    "`{service}`'s database could not be prepared for the build it would \
                     run, so nothing was started on it. It said:\n{said}"
                )
            })?;
        prepared(dir, &service, &id).map_err(|error| error.to_string())?;
        done.push(service);
    }
    Ok(done)
}

/// What the hub tells the service `id` about itself (`arkitekt_service.contract.facts.Facts`): the one
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
    let generated = hub_blocks(config, id, issued);
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
            .unwrap_or_default();
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
        // What a service said it offers, where it said — and nothing for one that said
        // nothing: no list here knows what a service is.
        assert_eq!(
            rekuest["peers"]["kraph"]["offers"]["rekuest_hook"].as_str(),
            Some("http://kraph:80/kraph/_hooks/rekuest")
        );
        assert!(rekuest["peers"]["mikro"]["offers"]
            .as_mapping()
            .is_some_and(|offers| offers.is_empty()));
        assert_eq!(
            rekuest["peers"]["mikro"]["url"].as_str(),
            Some("http://mikro:80/mikro")
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
