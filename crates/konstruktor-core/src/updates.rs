//! "Is there something newer upstream?" — answered without pulling.
//!
//! A pull is the honest way to find out, and the expensive one: it downloads the layers
//! whether or not anything changed. A registry answers the same question for free — the
//! digest a tag resolves to is in one response header — so the dashboard asks the
//! registry when it opens and only suggests a pull when the answer differs from what the
//! engine already holds.
//!
//! Only the digest is compared. Every registry that speaks the distribution API (Docker
//! Hub, GHCR, Quay, a private one) returns `Docker-Content-Digest` for a manifest request,
//! and that digest is exactly what the engine records in `RepoDigests` after a pull.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

use crate::config::hub::{HubConfig, DB_COMPOSE_SERVICE};
use crate::docker::ImageState;

const TIMEOUT: Duration = Duration::from_secs(10);

const ACCEPT: &str = "application/vnd.docker.distribution.manifest.list.v2+json, \
application/vnd.oci.image.index.v1+json, \
application/vnd.docker.distribution.manifest.v2+json, \
application/vnd.oci.image.manifest.v1+json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpstreamState {
    /// The tag upstream points at what the engine already has.
    Current,
    /// The tag has moved on; a pull would bring something new.
    Newer,
    /// Nothing pulled yet, so there is nothing to compare — a pull is due regardless.
    Missing,
    /// The registry could not be asked, or did not say. `error` explains.
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamCheck {
    pub service: String,
    pub image: String,
    pub state: UpstreamState,
    pub remote_digest: Option<String>,
    pub error: Option<String>,
}

/// One image reference, taken apart the way the engine does it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Reference {
    pub host: String,
    pub repository: String,
    pub tag: String,
}

/// `[host/]path[:tag]` — Docker's rules: the first component is a host only if it has a
/// dot or a port, or is `localhost`; a bare Hub name lives under `library/`.
pub(crate) fn parse(image: &str) -> Reference {
    // A `@sha256:…` pin cannot move, but it is not what the stack files write; strip it
    // so the tag lookup still works if someone does.
    let image = image.split('@').next().unwrap_or(image);

    let (mut path, tag) = match image.rsplit_once(':') {
        // `host:5000/repo` has a colon before a slash — that is a port, not a tag.
        Some((path, tag)) if !tag.contains('/') => (path.to_string(), tag.to_string()),
        _ => (image.to_string(), "latest".to_string()),
    };

    let mut host = "registry-1.docker.io".to_string();
    if let Some((first, rest)) = path.split_once('/') {
        if first.contains('.') || first.contains(':') || first == "localhost" {
            host = if first == "docker.io" {
                "registry-1.docker.io".to_string()
            } else {
                first.to_string()
            };
            path = rest.to_string();
        }
    }
    if host == "registry-1.docker.io" && !path.contains('/') {
        path = format!("library/{path}");
    }

    Reference {
        host,
        repository: path,
        tag,
    }
}

/// The digest `image` resolves to at its registry right now.
pub async fn remote_digest(image: &str) -> Result<String, String> {
    let reference = parse(image);
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!(
        "https://{}/v2/{}/manifests/{}",
        reference.host, reference.repository, reference.tag
    );

    let first = client
        .head(&url)
        .header("Accept", ACCEPT)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let response = if first.status() == reqwest::StatusCode::UNAUTHORIZED {
        let challenge = first
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| "registry asked for auth without saying how".to_string())?;
        let token = token(&client, challenge, &reference).await?;
        client
            .head(&url)
            .header("Accept", ACCEPT)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| e.to_string())?
    } else {
        first
    };

    if !response.status().is_success() {
        return Err(format!("registry answered {}", response.status()));
    }
    response
        .headers()
        .get("docker-content-digest")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| "registry did not report a digest".to_string())
}

/// Trades a `Bearer realm=…,service=…,scope=…` challenge for an anonymous pull token.
async fn token(
    client: &reqwest::Client,
    challenge: &str,
    reference: &Reference,
) -> Result<String, String> {
    let params = challenge
        .trim_start_matches("Bearer ")
        .split(',')
        .filter_map(|part| {
            let (key, value) = part.trim().split_once('=')?;
            Some((key.to_string(), value.trim_matches('"').to_string()))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let realm = params
        .get("realm")
        .ok_or_else(|| "auth challenge without a realm".to_string())?;
    let scope = params
        .get("scope")
        .cloned()
        .unwrap_or_else(|| format!("repository:{}:pull", reference.repository));

    let mut query = vec![("scope", scope)];
    if let Some(service) = params.get("service") {
        query.push(("service", service.clone()));
    }

    #[derive(Deserialize)]
    struct Token {
        token: Option<String>,
        access_token: Option<String>,
    }
    let issued: Token = client
        .get(realm)
        .query(&query)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    issued
        .token
        .or(issued.access_token)
        .ok_or_else(|| "auth server issued no token".to_string())
}

/// The digest an image reference pins itself to, if it names one.
fn pinned_digest(image: &str) -> Option<&str> {
    image.split_once('@').map(|(_, digest)| digest)
}

fn digest_of(repo_digest: &str) -> &str {
    repo_digest.rsplit('@').next().unwrap_or(repo_digest)
}

/// Every image the stack declares, checked against its registry, concurrently.
pub async fn check(images: &[ImageState]) -> Vec<UpstreamCheck> {
    let mut set = JoinSet::new();
    for (index, state) in images.iter().cloned().enumerate() {
        set.spawn(async move { (index, check_one(state).await) });
    }
    let mut results = Vec::new();
    while let Some(joined) = set.join_next().await {
        if let Ok(item) = joined {
            results.push(item);
        }
    }
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, check)| check).collect()
}

/// The local state of every image a deployment's stack declares.
///
/// The engine only — no network. Separate from [`for_deployment`] because a front end
/// showing which images are present refreshes far more often than it asks a registry
/// anything.
pub async fn images_for_deployment(dir: &std::path::Path) -> Result<Vec<ImageState>, String> {
    let profile = crate::profile::read_profile(dir).map_err(|e| e.to_string())?;
    crate::docker::image_states(&profile.config.stack_images()).await
}

/// Every image a deployment declares, checked against its registry.
///
/// One call because the two halves were being spelled out separately in each front end,
/// and the order matters: the local state is what the remote digests are compared to.
pub async fn for_deployment(dir: &std::path::Path) -> Result<Vec<UpstreamCheck>, String> {
    let images = images_for_deployment(dir).await?;
    let mut checks = check(&images).await;
    // A hub runs the build written down for it, not whatever its tag resolves to on this
    // machine: something else may have fetched the tag further since, and the hub is
    // behind its channel all the same.
    let pins = crate::lock::read(dir).pins;
    for found in &mut checks {
        behind_its_pin(found, &pins);
    }
    Ok(checks)
}

/// A check that found the local tag current, against the build the hub is pinned to: if
/// the registry serves another one, there is something newer to move to.
fn behind_its_pin(
    found: &mut UpstreamCheck,
    pins: &std::collections::BTreeMap<String, crate::lock::Pin>,
) {
    let pinned = pins
        .get(&found.service)
        .filter(|pin| pin.image == found.image)
        .and_then(|pin| pin.digest.as_deref());
    if let (UpstreamState::Current, Some(pinned), Some(remote)) =
        (&found.state, pinned, found.remote_digest.as_deref())
    {
        if pinned != remote {
            found.state = UpstreamState::Newer;
        }
    }
}

// --- advancing a pin -------------------------------------------------------------------
//
// An immutable reference never moves on its own — that is the whole point of it — so a
// pinned hub sits on the version it was created with until something deliberately moves
// it. `update` alone cannot: it pulls the reference the profile names, which resolves to
// what it always did. Advancing the pin means asking the repository what versions exist,
// picking one, and writing it into the profile.
//
// The rule for picking is deliberately narrow. Only the same major, only the same variant,
// and only forwards. A major is a migration — Postgres will not even open the old cluster
// — and it must be a decision somebody makes, not something an update offers because the
// number was bigger.

/// A tag read as a version: `16.13-1` is `[16, 13, 1]` with no variant, `8.11.0-alpine` is
/// `[8, 11, 0]` with the variant `alpine`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Version {
    parts: Vec<u64>,
    variant: String,
}

/// Reads a tag as a version, or decides it is not one.
///
/// A tag is a version if it *starts* with numbers: everything up to the first non-numeric
/// component is the version, and the rest is the variant that has to match for two tags to
/// be comparable at all. `latest`, `dev` and MinIO's `RELEASE.2025-02-18T16-25-55Z` are not
/// versions by this rule, which is correct — nothing here can order them, and offering to
/// move between them would be a guess.
pub(crate) fn version_of(tag: &str) -> Option<Version> {
    let mut parts = Vec::new();
    let mut rest = Vec::new();
    for component in tag.split(['.', '-']) {
        match component.parse::<u64>() {
            Ok(number) if rest.is_empty() => parts.push(number),
            _ => rest.push(component),
        }
    }
    (!parts.is_empty()).then(|| Version {
        parts,
        variant: rest.join("-"),
    })
}

/// The newest tag worth moving `current` to, out of everything the repository publishes.
///
/// `None` when there is nothing to move to, which is the ordinary answer: the hub is on
/// the newest, or its tag is not a version, or the only newer tags cross a major.
pub(crate) fn newer_version_tag<'a>(current: &str, available: &'a [String]) -> Option<&'a str> {
    let now = version_of(current)?;
    let major = *now.parts.first()?;

    available
        .iter()
        .filter_map(|tag| Some((tag, version_of(tag)?)))
        // Same variant, or a `16.13-1` hub would be offered `16.14-1-alpine`.
        .filter(|(_, candidate)| candidate.variant == now.variant)
        // Same major. Crossing one is a migration, not an update — see `guard`.
        .filter(|(_, candidate)| candidate.parts.first() == Some(&major))
        .filter(|(_, candidate)| candidate.parts > now.parts)
        .max_by(|(_, a), (_, b)| a.parts.cmp(&b.parts))
        .map(|(tag, _)| tag.as_str())
}

/// Every tag a repository publishes.
///
/// The same anonymous-token flow [`remote_digest`] uses, against the endpoint beside it.
pub async fn tags(image: &str) -> Result<Vec<String>, String> {
    let reference = parse(image);
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!(
        "https://{}/v2/{}/tags/list",
        reference.host, reference.repository
    );

    let first = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let response = if first.status() == reqwest::StatusCode::UNAUTHORIZED {
        let challenge = first
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| "registry asked for auth without saying how".to_string())?;
        let token = token(&client, challenge, &reference).await?;
        client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| e.to_string())?
    } else {
        first
    };

    if !response.status().is_success() {
        return Err(format!("registry answered {}", response.status()));
    }

    #[derive(Deserialize)]
    struct Tags {
        #[serde(default)]
        tags: Vec<String>,
    }
    let listed: Tags = response.json().await.map_err(|e| e.to_string())?;
    Ok(listed.tags)
}

/// One service's pin, and the version it could be moved to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Advance {
    pub service: String,
    /// The reference the profile names now.
    pub from: String,
    /// The reference it would name instead.
    pub to: String,
}

/// What advancing this image would move it to, asked of its registry.
///
/// The digest is dropped on the way: a hub moving from `daten:16.13-1@sha256:…` to
/// `daten:16.14-1` is moving to a version, and re-pinning it to a digest here would record
/// what today's registry happens to serve rather than what was chosen.
pub async fn advance_for(service: &str, image: &str) -> Option<Advance> {
    let bare = image.split('@').next().unwrap_or(image);
    let (repository, tag) = bare.rsplit_once(':')?;
    let available = tags(bare).await.ok()?;
    let newer = newer_version_tag(tag, &available)?;
    Some(Advance {
        service: service.to_string(),
        from: image.to_string(),
        to: format!("{repository}:{newer}"),
    })
}

/// Every infrastructure image with a newer version of itself published.
pub async fn advances(config: &HubConfig) -> Vec<Advance> {
    let mut found = Vec::new();
    for (service, image) in config.stack_images() {
        if !is_infrastructure(config, &service) {
            continue;
        }
        if let Some(advance) = advance_for(&service, &image).await {
            found.push(advance);
        }
    }
    found
}

/// Whether a compose service is infrastructure rather than one of the hub's own services.
///
/// The distinction matters because the two carry different risk. An Arkitekt service is a
/// Django application that migrates its own schema forward on start; the infrastructure is
/// the database, the object store, the gateway and the cache, where a moved image can mean
/// a cluster the new binary refuses to open. So `update` moves services by default and
/// takes `--infra` to be told to move the rest.
///
/// Derived from the profile rather than a list of names: everything `stack_images` emits
/// that is not an enabled service's host is infrastructure, so a service added upstream is
/// classified correctly without this having to be edited.
pub fn is_infrastructure(config: &HubConfig, service: &str) -> bool {
    // The health reporter holds no data and follows `latest`: moving it risks nothing the
    // infrastructure's caution exists for.
    let reporter = config.reporter.as_ref().is_some_and(|r| r.host == service);
    // takt is Rekuest's other half: it moves with Rekuest, at Rekuest's risk.
    let companion = crate::generate::compose::companion_of(config, service).is_some();
    !reporter
        && !companion
        && !config
            .enabled_services()
            .into_iter()
            .any(|id| config.service(id).host == service)
}

/// What has to be said before one service's image is allowed to move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "verdict", content = "detail")]
pub enum Guard {
    /// Nothing in the way.
    Clear,
    /// Recreating this service on the image now on disk would break it.
    Refuse(String),
    /// Something could not be checked. Worth saying, not worth stopping for.
    Warn(String),
}

/// Whether recreating `service` on the image currently on disk is safe.
///
/// Only the database has an answer other than [`Guard::Clear`], and it is the one that
/// matters: Postgres will not open a cluster written by a different major — there is no
/// in-place upgrade — so a `db` image that moved from 16 to 17 leaves the container
/// crash-looping and every service behind it unable to connect, with the previous image
/// already replaced.
///
/// **This must be asked after the pull and before the recreate.** The major it reads is
/// the one the *local* image declares, and before a pull that is still the image already
/// running, which agrees with the data by construction and would make the check inert.
/// Pulling first is harmless — a fetched image changes nothing until a container is
/// recreated on it — so the safe order is pull, ask, then recreate or refuse.
pub async fn guard(dir: &std::path::Path, config: &HubConfig, service: &str) -> Guard {
    guard_on(dir, config, service, &config.db.image).await
}

/// [`guard`], of the image that would actually be run. An update asks about what its
/// channel just fetched; a start asks about the build written down for the hub
/// ([`pinned_database`]), which is not what the tag resolves to once something else on
/// the machine has fetched it further.
pub async fn guard_on(
    dir: &std::path::Path,
    config: &HubConfig,
    service: &str,
    image: &str,
) -> Guard {
    if service != DB_COMPOSE_SERVICE {
        return Guard::Clear;
    }
    let data = crate::backup::live_pgdata_major(dir, config).await;
    let server = crate::docker::image_pg_major(image).await;
    match crate::backup::major_move(data, server) {
        crate::backup::MajorMove::Same(_) => Guard::Clear,
        crate::backup::MajorMove::Across { data, server } => Guard::Refuse(format!(
            "the database image is now Postgres {server}, and this hub's data was written \
             by Postgres {data}. Postgres will not open a cluster from another major, so \
             recreating `{service}` would leave it crash-looping. Moving majors is a \
             migration: back the hub up, then restore the dump into the new version."
        )),
        crate::backup::MajorMove::Unknown => Guard::Warn(
            "the Postgres major on one of the two sides could not be read — this hub's own \
             data, or the image's `PG_MAJOR` — so whether the new image can open the \
             existing cluster is unverified"
                .to_string(),
        ),
    }
}

/// Where a backup taken before an update goes unless somebody says otherwise: a
/// `konstruktor-backups` folder beside the deployment, so it survives the deployment.
pub fn default_backup_folder(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    dir.parent()
        .map(|parent| parent.join("konstruktor-backups"))
}

/// What the infrastructure could move to: images whose tag moved upstream, and pins with a
/// newer version published. Held back from ordinary updates — see [`is_infrastructure`].
#[derive(Debug, Clone, Default, Serialize)]
pub struct InfrastructureUpdates {
    /// Compose services whose image moved, or was never pulled.
    pub moved: Vec<String>,
    pub advances: Vec<Advance>,
}

impl InfrastructureUpdates {
    pub fn is_empty(&self) -> bool {
        self.moved.is_empty() && self.advances.is_empty()
    }
}

/// Asks the registries what the infrastructure of the hub in `dir` could move to.
pub async fn infrastructure(dir: &std::path::Path) -> Result<InfrastructureUpdates, String> {
    let config = crate::profile::read_profile(dir)
        .map_err(|e| e.to_string())?
        .config;
    let checks = for_deployment(dir).await?;
    Ok(InfrastructureUpdates {
        moved: checks
            .into_iter()
            .filter(|c| matches!(c.state, UpstreamState::Newer | UpstreamState::Missing))
            .filter(|c| is_infrastructure(&config, &c.service))
            .map(|c| c.service)
            .collect(),
        advances: advances(&config).await,
    })
}

/// What to update, and how carefully. Both front ends build one of these: the CLI's
/// `update` from its flags, the dashboard's per-service button from the card it sits on.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateRequest {
    /// Compose services to recreate, in order.
    pub services: Vec<String>,
    /// Pins to move first. The profile is rewritten before any pull, since it decides
    /// which image the pull fetches; every advanced service is recreated too.
    #[serde(default)]
    pub advances: Vec<Advance>,
    /// Fetch the image before recreating. Off only for an image that was already pulled —
    /// applying it must work with the registry unreachable.
    pub pull: bool,
    /// Back the hub up into this folder first. Migrations run when a service starts and
    /// are one-way, so this is the only way back.
    #[serde(default)]
    pub backup_into: Option<std::path::PathBuf>,
    /// Ask every service whether it still answers, afterwards.
    pub health_check: bool,
}

/// What an update is doing, as it does it.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum UpdateEvent {
    Step {
        title: String,
    },
    /// A line of output from compose, the backup or the health check.
    Line {
        line: String,
        stderr: bool,
    },
    Warning {
        message: String,
    },
    /// This service was left alone: recreating it would break it.
    Refused {
        service: String,
        reason: String,
    },
    Updated {
        service: String,
    },
}

/// What an update did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateReport {
    /// Where the backup taken first went.
    pub backup: Option<String>,
    pub updated: Vec<String>,
    pub refused: Vec<(String, String)>,
    /// The layout moves this update took the hub's files through.
    pub migrated: Vec<crate::migrate::Step>,
    /// The generated files whose contents changed.
    pub rewritten: Vec<String>,
    /// Layout moves whose closing commands failed: the hub runs the new files, and the
    /// next update runs those commands again before anything else.
    pub unfinished: Vec<crate::migrate::Step>,
    /// Present when a health check was asked for.
    pub health: Option<Vec<crate::health::ServiceHealth>>,
}

impl UpdateReport {
    /// Everything asked for was updated, and — when checked — everything answers.
    pub fn succeeded(&self) -> bool {
        self.refused.is_empty()
            && self.unfinished.is_empty()
            && !self.updated.is_empty()
            && self
                .health
                .as_ref()
                .is_none_or(|health| health.iter().all(|s| s.healthy))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error(transparent)]
    Profile(#[from] crate::profile::ProfileError),
    #[error(transparent)]
    Backup(#[from] crate::backup::BackupError),
    #[error("{0}")]
    Compose(String),
    #[error("{0}")]
    Health(String),
    /// The hub is frozen and the update would have to move what is held. See
    /// [`crate::freeze`].
    #[error("{0}")]
    Frozen(String),
    /// A command a move needs run failed: a layout step's ([`crate::migrate`]), or a
    /// service's own upgrade.
    #[error("{0}")]
    Migration(String),
    /// A service's image would not write its config for this hub ([`crate::contract`]).
    #[error(transparent)]
    Config(#[from] crate::contract::RenderError),
}

/// What a service's image says of the config it is about to be started on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reads {
    /// It reads every key as written (one under a former name included: a release may
    /// rename a key within its major, and says so itself).
    Yes,
    /// It does not: the config sets keys it does not read. With what it printed.
    No(String),
    /// It was not asked: an image from before the question, or one that is no Arkitekt
    /// service. Such an image is run as it always was.
    Unasked,
    /// It was asked and the asking failed — the command crashed, the config is invalid to
    /// it, the container did not start. With what it printed. Not a refusal: whatever is
    /// wrong will be loud when the service starts, which a key nobody reads never is.
    Failed(String),
}

/// `validate_settings --strict`'s own no (sysexits' `EX_CONFIG`).
const NOT_READ: i32 = 78;

fn last_lines(output: &str) -> String {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines[lines.len().saturating_sub(12)..].join("\n")
}

/// Reads the answer off `manage.py validate_settings --strict`: 0 is yes, 78 the command's
/// own no. 2 is argparse not knowing the command or the flag, 126 and 127 a container with
/// no `python`: the question was never put. Anything else is the asking going wrong.
pub fn reads_from(code: Option<i32>, output: &str) -> Reads {
    match code {
        Some(0) => Reads::Yes,
        Some(NOT_READ) => Reads::No(last_lines(output)),
        Some(2 | 126 | 127) | None => Reads::Unasked,
        Some(_) => Reads::Failed(last_lines(output)),
    }
}

/// Asks the image `service` would be recreated onto whether it reads the config generated
/// for it, in a container of its own that touches nothing: no dependencies started, no
/// database asked.
pub async fn reads_its_config(dir: &std::path::Path, service: &str) -> Reads {
    let output = crate::engine_probe::engine()
        .async_command()
        .args([
            "compose",
            "run",
            "--rm",
            "--no-deps",
            "-T",
            service,
            "python",
            "manage.py",
            "validate_settings",
            "--strict",
        ])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .await;
    match output {
        Ok(out) => reads_from(
            out.status.code(),
            &format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        ),
        Err(_) => Reads::Unasked,
    }
}

/// The database image a start of the hub in `dir` runs: the build written down for it, or
/// what the profile names when there is none.
pub fn pinned_database(dir: &std::path::Path, config: &HubConfig) -> String {
    crate::pins::references(config, &crate::lock::read(dir).pins)
        .remove(DB_COMPOSE_SERVICE)
        .unwrap_or_else(|| config.db.image.clone())
}

/// Whether any container of the hub in `dir` is running.
async fn running(dir: &std::path::Path) -> bool {
    crate::engine_probe::engine()
        .async_command()
        .args(["compose", "ps", "--status", "running", "-q"])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .is_ok_and(|out| !String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

/// The label release pipelines put a version under.
const VERSION_LABEL: &str = "org.opencontainers.image.version";

/// The version of the build `service`'s container was created from, if it has a container
/// and the image says.
async fn running_version(dir: &std::path::Path, service: &str) -> Option<String> {
    let engine = crate::engine_probe::engine();
    let container = engine
        .async_command()
        .args(["compose", "ps", "--all", "-q", service])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    let container = String::from_utf8_lossy(&container.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    let image = engine
        .async_command()
        .args(["inspect", "--format", "{{.Image}}", &container])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    let image = String::from_utf8_lossy(&image.stdout).trim().to_string();
    crate::docker::image_label(&image, VERSION_LABEL).await
}

/// What a service's new release did with `manage.py upgrade`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upgraded {
    /// It ran, or had nothing to do.
    Done,
    /// The release ships no such command: nothing of its own to do between versions.
    None,
    /// It failed, with what it printed.
    Failed(String),
}

/// Reads the answer off `manage.py upgrade`: 0 is done. 2 is argparse not knowing the
/// command, 126 and 127 a container with no `python`: there is no upgrade to run.
/// Anything else is the upgrade failing.
pub fn upgraded_from(code: Option<i32>, output: &str) -> Upgraded {
    match code {
        Some(0) => Upgraded::Done,
        Some(2 | 126 | 127) => Upgraded::None,
        _ => Upgraded::Failed(last_lines(output)),
    }
}

/// Whether the release `service` is about to run ships an upgrade command at all. Asked
/// while the old container still serves: only a release that has one costs the service a
/// stop.
async fn ships_an_upgrade(dir: &std::path::Path, service: &str) -> bool {
    crate::engine_probe::engine()
        .async_command()
        .args([
            "compose",
            "run",
            "--rm",
            "--no-deps",
            "-T",
            service,
            "python",
            "manage.py",
            "help",
            "upgrade",
        ])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .is_ok_and(|out| out.status.success())
}

/// The new release's database migrations, as a command of their own in a container of the
/// new image. The services also migrate when they start, which then finds nothing to do.
fn migrate(service: &str) -> Vec<String> {
    [
        "compose",
        "run",
        "--rm",
        "--no-deps",
        "-T",
        service,
        "python",
        "manage.py",
        "migrate",
        "--noinput",
    ]
    .map(String::from)
    .to_vec()
}

/// Runs the new release's own upgrade — what it has to do to its data between the two
/// versions, which only it knows — in a container of the new image. The service's own
/// container is stopped by the caller: the old code must not be writing meanwhile.
async fn upgrade(dir: &std::path::Path, service: &str, from: &str, to: &str) -> Upgraded {
    let output = crate::engine_probe::engine()
        .async_command()
        .args([
            "compose",
            "run",
            "--rm",
            "--no-deps",
            "-T",
            service,
            "python",
            "manage.py",
            "upgrade",
            "--from",
            from,
            "--to",
            to,
        ])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .await;
    match output {
        Ok(out) => upgraded_from(
            out.status.code(),
            &format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        ),
        Err(error) => Upgraded::Failed(error.to_string()),
    }
}

/// What an update would do to one service, worked out without changing the hub.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ServicePreview {
    pub service: String,
    /// The version it runs, and the one its channel points at now, where the images say.
    pub from: Option<String>,
    pub to: Option<String>,
    /// Why the update would leave it where it is, if it would.
    pub refused: Option<String>,
    /// The keys of its config that would be written differently, as dotted paths. Never
    /// their values: a config holds the service's secrets.
    pub config_changes: Vec<String>,
    /// The database migrations its new release would apply, in order.
    pub migrations: Vec<String>,
    /// What could not be worked out, and why.
    pub notes: Vec<String>,
}

/// The dotted paths at which two config documents differ.
pub fn changed_keys(before: &serde_norway::Value, after: &serde_norway::Value) -> Vec<String> {
    fn walk(
        before: Option<&serde_norway::Value>,
        after: Option<&serde_norway::Value>,
        path: &str,
        out: &mut Vec<String>,
    ) {
        match (before, after) {
            (
                Some(serde_norway::Value::Mapping(before)),
                Some(serde_norway::Value::Mapping(after)),
            ) => {
                let mut keys: Vec<&serde_norway::Value> =
                    before.keys().chain(after.keys()).collect();
                keys.sort_by_key(|key| key.as_str().unwrap_or_default().to_string());
                keys.dedup();
                for key in keys {
                    let name = key.as_str().unwrap_or("?");
                    let inner = match path.is_empty() {
                        true => name.to_string(),
                        false => format!("{path}.{name}"),
                    };
                    walk(before.get(key), after.get(key), &inner, out);
                }
            }
            (before, after) if before != after => out.push(match (before, after) {
                (None, _) => format!("{path} (new)"),
                (_, None) => format!("{path} (gone)"),
                _ => path.to_string(),
            }),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(Some(before), Some(after), "", &mut out);
    out
}

/// The migrations `manage.py migrate --plan` lists: the names it prints at the margin,
/// under which it indents what each does.
pub fn planned_migrations(output: &str) -> Vec<String> {
    output
        .lines()
        .skip_while(|line| !line.starts_with("Planned operations"))
        .skip(1)
        .filter(|line| !line.starts_with(' ') && !line.trim().is_empty())
        .filter(|line| line.contains('.') && !line.contains(' '))
        .map(|line| line.trim().to_string())
        .collect()
}

/// What updating `services` would do, without doing it.
///
/// The one thing this changes is which images are on the machine: each service's channel
/// is fetched, since a release cannot be asked anything before it is here. The hub keeps
/// running the builds written down for it. Then each new release is asked what an update
/// would ask it — whether it can be moved to from what runs, beside what else would run —
/// and has its config written for it into a scratch file and its migrations listed against
/// the running database, neither of which touches the hub's own files or data.
pub async fn preview(
    dir: &std::path::Path,
    services: &[String],
    on_event: &(dyn Fn(UpdateEvent) + Send + Sync),
) -> Result<Vec<ServicePreview>, UpdateError> {
    Box::pin(preview_on(dir, services, on_event)).await
}

async fn preview_on(
    dir: &std::path::Path,
    services: &[String],
    on_event: &(dyn Fn(UpdateEvent) + Send + Sync),
) -> Result<Vec<ServicePreview>, UpdateError> {
    let step = |title: String| on_event(UpdateEvent::Step { title });
    let line = |l: crate::compose::ComposeLine| {
        on_event(UpdateEvent::Line {
            line: l.line,
            stderr: l.stderr,
        })
    };
    let mut config = crate::profile::read_profile(dir)?.config;
    // What a layout move would bring along is part of what is previewed.
    if !crate::migrate::pending(dir, &config).is_empty() {
        for (service, image) in crate::migrate::caught_up_images(&config) {
            config.set_service_image(&service, &image);
        }
    }
    let channels: std::collections::BTreeMap<String, String> =
        config.stack_images().into_iter().collect();
    let built_here: Vec<String> = crate::docker::image_states(&config.stack_images())
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|state| state.present && state.repo_digests.is_empty())
        .map(|state| state.service)
        .collect();
    let is_service = |name: &str| {
        config
            .enabled_services()
            .into_iter()
            .find(|id| config.service(*id).host == name)
    };

    // --- fetch ---------------------------------------------------------------------------
    let mut previews: Vec<ServicePreview> = Vec::new();
    for service in services {
        let mut said = ServicePreview {
            service: service.clone(),
            ..ServicePreview::default()
        };
        let companions = crate::generate::compose::companions(&config, service);
        for name in std::iter::once(service).chain(&companions) {
            let Some(image) = channels.get(name).filter(|_| !built_here.contains(name)) else {
                continue;
            };
            step(format!("Fetching {name}"));
            let pull = vec!["pull".to_string(), image.clone()];
            if let Err(error) = crate::compose::run_streamed(dir, pull, &line).await {
                said.notes.push(format!(
                    "`{name}` could not be fetched, so nothing more is known: {}",
                    last_lines(&error)
                ));
            }
        }
        said.from = running_version(dir, service).await;
        said.to = match channels.get(service) {
            Some(image) => crate::docker::image_label(image, VERSION_LABEL).await,
            None => None,
        };
        previews.push(said);
    }

    // --- ask each release ------------------------------------------------------------------
    let mut reached: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for (service, _) in config.stack_images() {
        let version = match previews.iter().find(|said| said.service == service) {
            Some(said) => said.to.clone(),
            None => running_version(dir, &service).await,
        };
        if let Some(version) = version {
            reached.insert(service, version);
        }
    }
    let identity = crate::credentials::read_credentials(dir)
        .map(|credentials| credentials.issued_identity())
        .unwrap_or_default();
    let mut described = crate::contract::described(dir, &config).await;
    let mut asked: Vec<(String, crate::catalog::ServiceId, String)> = Vec::new();
    for said in &previews {
        let (Some(id), Some(image)) = (is_service(&said.service), channels.get(&said.service))
        else {
            continue;
        };
        match crate::contract::describe(image).await {
            Some(description) => {
                described.insert(said.service.clone(), description);
                asked.push((said.service.clone(), id, image.clone()));
            }
            None => {
                described.remove(&said.service);
            }
        }
    }
    let scratch = dir.join(".konstruktor").join("preview");
    for said in &mut previews {
        let Some((_, id, image)) = asked.iter().find(|(name, _, _)| *name == said.service) else {
            if is_service(&said.service).is_some() && said.notes.is_empty() {
                said.notes.push(
                    "its image does not describe itself, so its config stays the one \
                     generated for it and is not previewed"
                        .to_string(),
                );
            }
            continue;
        };
        let description = &described[&said.service];
        if let (Some(oldest), Some(from)) =
            (description.upgrade_from.as_deref(), said.from.as_deref())
        {
            if !crate::contract::at_least(from, oldest) {
                said.refused = Some(format!(
                    "it runs {from}, and this release can only be moved to from {oldest} or \
                     newer: it has to stop at a release in between first"
                ));
            }
        }
        if said.refused.is_none() {
            said.refused = crate::contract::unmet(description, &reached);
        }

        // Its config, as this release would write it — into a scratch file, never over
        // the one the running service reads.
        let facts =
            crate::generate::dump(&crate::contract::facts(&config, *id, &identity, &described));
        let facts_file = scratch.join(format!("{}.facts.yaml", said.service));
        let written =
            std::fs::create_dir_all(&scratch).and_then(|()| std::fs::write(&facts_file, &facts));
        if let Err(error) = written {
            said.notes
                .push(format!("its config could not be previewed: {error}"));
            continue;
        }
        let overrides = crate::overrides::path(dir, &said.service);
        let rendered = match crate::contract::render(image, &facts_file, &overrides).await {
            crate::contract::Rendered::Config(text) => text,
            crate::contract::Rendered::Refused(why) => {
                said.refused.get_or_insert(why);
                continue;
            }
            crate::contract::Rendered::Failed(why) => {
                said.notes
                    .push(format!("its config could not be previewed: {why}"));
                continue;
            }
        };
        let current = std::fs::read_to_string(dir.join(format!("configs/{}.yaml", said.service)))
            .ok()
            .and_then(|text| serde_norway::from_str::<serde_norway::Value>(&text).ok());
        if let (Some(current), Ok(new)) = (
            current,
            serde_norway::from_str::<serde_norway::Value>(&rendered),
        ) {
            said.config_changes = changed_keys(&current, &new);
        }

        // Its migrations, against the database as it is: read, not applied. Run where the
        // service's container is, on the config just written for the new release.
        let config_file = scratch.join(format!("{}.yaml", said.service));
        let networks = match std::fs::write(&config_file, &rendered) {
            Ok(()) => container_networks(dir, &said.service).await,
            Err(_) => Vec::new(),
        };
        let Some(network) = networks.first() else {
            said.notes.push(
                "its pending migrations are not listed: the hub is not running, and they \
                 are read off its database"
                    .to_string(),
            );
            continue;
        };
        let absolute = std::fs::canonicalize(&config_file).unwrap_or(config_file.clone());
        let plan = crate::engine_probe::engine()
            .async_command()
            .args([
                "run",
                "--rm",
                "--network",
                network,
                "-v",
                &format!("{}:/workspace/config.yaml:ro", absolute.to_string_lossy()),
                image,
                "python",
                "manage.py",
                "migrate",
                "--plan",
            ])
            .stdin(std::process::Stdio::null())
            .output()
            .await;
        match plan {
            Ok(out) if out.status.success() => {
                said.migrations = planned_migrations(&String::from_utf8_lossy(&out.stdout));
            }
            Ok(out) => said.notes.push(format!(
                "its pending migrations could not be listed: {}",
                last_lines(&String::from_utf8_lossy(&out.stderr))
            )),
            Err(error) => said.notes.push(format!(
                "its pending migrations could not be listed: {error}"
            )),
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(previews)
}

/// The networks `service`'s container is attached to; empty when it has none.
async fn container_networks(dir: &std::path::Path, service: &str) -> Vec<String> {
    let engine = crate::engine_probe::engine();
    let Some(container) = engine
        .async_command()
        .args(["compose", "ps", "--status", "running", "-q", service])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()
        .and_then(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .map(|line| line.trim().to_string())
        })
        .filter(|id| !id.is_empty())
    else {
        return Vec::new();
    };
    engine
        .async_command()
        .args([
            "inspect",
            "--format",
            "{{range $name, $_ := .NetworkSettings.Networks}}{{$name}}\n{{end}}",
            &container,
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Applies an update: back up, record what is running and keep a copy of the files, fetch
/// what each service's channel points at now, write those builds and the files, and only
/// then recreate the services, record again, and check it all came back.
///
/// The one sequence both front ends run — it used to be written twice, and the dashboard's
/// copy took no backup and never asked whether the services survived.
///
/// **This is the one thing that changes a build.** A hub's compose file names the exact
/// build of every image ([`crate::pins`]); here a channel is looked at again, and the build
/// it points at now is written down and run.
///
/// **The files move with the images.** They are regenerated from the profile on every
/// update, so what a release of a service reads is written before its image runs. A hub
/// whose files are of an older layout ([`crate::migrate`]) has every service moved, asked
/// for or not: files of one layout and images of another are the hub that answers its
/// health checks and does nothing.
///
/// **Nothing is replaced until everything has arrived.** Until the first container is
/// recreated a failure — an image that will not pull, a profile that will not generate, a
/// release that does not read its config, a command a move needs first — puts the files
/// and the builds back as they were and leaves the hub as it found it. After that an
/// update is not undone by itself: the services have migrated their databases, and
/// `rollback` (which puts the files back too) says what that means.
///
/// A refused service is reported and skipped, not an error: the rest still move. The
/// caller decides what a partial update means for its exit code.
pub async fn apply(
    dir: &std::path::Path,
    request: &UpdateRequest,
    on_event: &(dyn Fn(UpdateEvent) + Send + Sync),
) -> Result<UpdateReport, UpdateError> {
    // On the heap: the sequence is long, and its state does not fit a caller's stack.
    Box::pin(apply_on(dir, request, on_event)).await
}

async fn apply_on(
    dir: &std::path::Path,
    request: &UpdateRequest,
    on_event: &(dyn Fn(UpdateEvent) + Send + Sync),
) -> Result<UpdateReport, UpdateError> {
    use crate::lock;
    use crate::migrate::When;

    let mut report = UpdateReport::default();
    let step = |title: String| on_event(UpdateEvent::Step { title });
    let warn = |message: String| on_event(UpdateEvent::Warning { message });
    let line = |l: crate::compose::ComposeLine| {
        on_event(UpdateEvent::Line {
            line: l.line,
            stderr: l.stderr,
        })
    };
    // One of a move's commands, in the hub's folder.
    let run_action = |action: &crate::migrate::Action| {
        step(action.title.clone());
        crate::compose::run_streamed(dir, action.docker.clone(), &line)
    };

    // --- is it held? ----------------------------------------------------------------
    // Before anything, the backup included: a hub whose files have to move takes every
    // service along, and a frozen service is one that was told to stay.
    let frozen = crate::freeze::frozen(dir);
    if !frozen.is_empty() {
        let config = crate::profile::read_profile(dir)?.config;
        if let Some(moves) = crate::migrate::pending(dir, &config).first() {
            return Err(UpdateError::Frozen(format!(
                "this hub is frozen ({}), and its files are from an earlier Konstruktor: \
                 bringing them up to date ({}) moves every service with them. Run \
                 `konstruktor unfreeze` first, then `konstruktor update`; `konstruktor \
                 freeze` holds it again afterwards. Nothing was changed.",
                frozen.keys().cloned().collect::<Vec<_>>().join(", "),
                moves.title
            )));
        }
    }

    // --- what an earlier update still owes ---------------------------------------------
    // Its files are in place and its services run; the commands that close the move did
    // not all succeed. Nothing else happens until they have.
    for owed in crate::migrate::unfinished(dir) {
        step(format!("Finishing an earlier update: {}", owed.title));
        for action in owed.at(When::After) {
            run_action(action).await.map_err(|error| {
                UpdateError::Migration(format!(
                    "the last update is still unfinished: \"{}\" failed again ({error}). \
                     Nothing else was changed.",
                    action.title
                ))
            })?;
        }
        crate::migrate::finish(dir, owed.to).map_err(crate::profile::ProfileError::Io)?;
    }

    // --- the backup ----------------------------------------------------------------
    if let Some(into) = &request.backup_into {
        step(format!("Backing up into {} first", into.to_string_lossy()));
        let backup = crate::backup::run(
            &crate::backup::BackupRequest {
                dir: dir.to_path_buf(),
                target: into.clone(),
            },
            &|event| match event {
                crate::backup::BackupEvent::Step { title, .. } => step(title),
                crate::backup::BackupEvent::Line { line, stderr, .. } => {
                    on_event(UpdateEvent::Line { line, stderr })
                }
                crate::backup::BackupEvent::Skipped { reason, .. } => warn(reason),
            },
        )
        .await?;
        for warning in backup.warnings {
            warn(warning);
        }
        report.backup = Some(backup.path);
    }

    // --- what is running now, and on which files -------------------------------------
    // What `rollback` reads. Not fatal: an unwritable lock costs the way back, which is
    // worth saying, and is no reason to refuse an update somebody asked for.
    let mut config = crate::profile::read_profile(dir)?.config;
    let at = lock::now();
    if let Err(error) = lock::record(dir, &config, "before update", at).await {
        warn(format!(
            "could not record what this hub is running ({error}) — rollback will have \
             nothing to go back to"
        ));
    }
    if let Some(backup) = &report.backup {
        let _ = lock::attach_backup(dir, backup);
    }
    let kept = match crate::generations::take(dir, at) {
        Ok(name) => {
            let _ = lock::attach_files(dir, &name);
            Some(name)
        }
        Err(error) => {
            warn(format!(
                "could not keep a copy of this hub's files ({error}) — a failed update \
                 will not be able to put them back"
            ));
            None
        }
    };
    // Puts the files and the builds back as they were: for a failure before anything was
    // recreated.
    let put_back = |why: &str| {
        if let Some(name) = &kept {
            match crate::generations::restore(dir, name) {
                Ok(()) => warn(format!("{why}; this hub's files are as they were")),
                Err(error) => warn(format!(
                    "{why}, and the files could not be put back ({error}) — they are in {}",
                    crate::generations::path(dir, name).to_string_lossy()
                )),
            }
        }
    };

    // --- what moves --------------------------------------------------------------------
    let mut services = request.services.clone();
    let pending = crate::migrate::pending(dir, &config);
    for moved in &pending {
        step(format!("Moving this hub's files: {}", moved.title));
    }
    if !pending.is_empty() {
        // Every service reads the new files, so every service is of the release they
        // were written for. The infrastructure is not: its images are held back as ever.
        for (service, _) in config.stack_images() {
            if !is_infrastructure(&config, &service) && !services.contains(&service) {
                services.push(service);
            }
        }
    }
    let edited = crate::migrate::hand_edited(dir);
    if let (false, Some(name)) = (edited.is_empty(), &kept) {
        warn(format!(
            "{} changed by hand since generated; the files are written again, and yours \
             are kept in {}",
            edited.join(", "),
            crate::generations::path(dir, name).to_string_lossy()
        ));
    }
    // A frozen service's pin is not advanced; it is said where it is skipped, below.
    let advances: Vec<&Advance> = request
        .advances
        .iter()
        .filter(|a| !frozen.contains_key(&a.service))
        .collect();
    let mut images: Vec<(String, String)> = advances
        .iter()
        .map(|a| (a.service.clone(), a.to.clone()))
        .collect();
    if !pending.is_empty() {
        // The files are written for one major of each service; a service still on what an
        // earlier Konstruktor seeded follows them there.
        let runs = |service: &str| {
            config
                .enabled_services()
                .into_iter()
                .any(|id| config.service(id).host == service)
        };
        for (service, image) in crate::migrate::caught_up_images(&config) {
            // A service that is switched off moves too, unsaid: it starts there if added.
            if runs(&service) {
                step(format!("{service} follows {image}"));
            }
            images.push((service, image));
        }
        for (service, image) in crate::migrate::unsupported_images(&config) {
            warn(format!(
                "`{service}` runs {image}, which somebody chose and this Konstruktor was \
                 not written for — it is left on it, and whether it reads the rewritten \
                 files is not known"
            ));
        }
    }
    // It is the profile that moved, not the tag: these have to be recreated even though
    // the registry said their old reference was current.
    for advance in &advances {
        if !services.contains(&advance.service) {
            services.push(advance.service.clone());
        }
    }
    // The profile as it will be written: the channels everything below is fetched by.
    for (service, image) in &images {
        config.set_service_image(service, image);
    }
    if !advances.is_empty() {
        step(format!(
            "Profile moves to {}",
            advances
                .iter()
                .map(|a| a.to.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let channels: std::collections::BTreeMap<String, String> =
        config.stack_images().into_iter().collect();

    // --- fetch, and ask whether what arrived may be run -------------------------------
    // By channel, not through the compose file: that names the build the hub runs, and
    // fetching it again would find nothing new. An image that is on this machine and came
    // from no registry (built here) is not asked for anywhere.
    let built_here: Vec<String> = crate::docker::image_states(&config.stack_images())
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|state| state.present && state.repo_digests.is_empty())
        .map(|state| state.service)
        .collect();
    let mut moving: Vec<(String, Vec<String>)> = Vec::new();
    for service in &services {
        // takt moves with Rekuest, below; asked for beside it, it is not moved twice.
        if crate::generate::compose::companion_of(&config, service)
            .is_some_and(|of| services.contains(&of))
        {
            continue;
        }
        // Held on the build it runs: not fetched, not recreated, and said.
        if let Some(reason) = crate::freeze::reason(&frozen, service) {
            on_event(UpdateEvent::Refused {
                service: service.clone(),
                reason: reason.clone(),
            });
            report.refused.push((service.clone(), reason));
            continue;
        }
        // takt has an image of its own, released with Rekuest's under the same tag, so it
        // is fetched with it.
        let companions: Vec<String> = crate::generate::compose::companions(&config, service);
        if request.pull {
            step(format!("Fetching {service}"));
            for name in std::iter::once(service).chain(&companions) {
                let Some(image) = channels.get(name).filter(|_| !built_here.contains(name)) else {
                    continue;
                };
                let pull = vec!["pull".to_string(), image.clone()];
                if let Err(error) = crate::compose::run_streamed(dir, pull, &line).await {
                    put_back(&format!("`{name}` could not be fetched"));
                    return Err(UpdateError::Compose(error));
                }
            }
        }
        // The guard reads what the *new* image declares, so it is asked after the fetch —
        // and before the build is written down: a refused service keeps the one it runs.
        match guard(dir, &config, service).await {
            Guard::Refuse(reason) => {
                on_event(UpdateEvent::Refused {
                    service: service.clone(),
                    reason: reason.clone(),
                });
                report.refused.push((service.clone(), reason));
                continue;
            }
            Guard::Warn(detail) => warn(detail),
            Guard::Clear => {}
        }
        moving.push((service.clone(), companions));
    }

    // --- may each release be moved to, and beside the others? ----------------------------
    // What a release says of itself: the oldest version it can be moved to from directly,
    // and the versions of other services it needs beside it. Asked of what was just
    // fetched, before any of it is written down.
    let mut reached: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for (service, _) in config.stack_images() {
        let moves = moving.iter().any(|(name, _)| *name == service);
        let version = match (moves, channels.get(&service)) {
            (true, Some(image)) => crate::docker::image_label(image, VERSION_LABEL).await,
            _ => running_version(dir, &service).await,
        };
        if let Some(version) = version {
            reached.insert(service, version);
        }
    }
    let mut allowed: Vec<(String, Vec<String>)> = Vec::new();
    for (service, companions) in moving {
        let said = match channels.get(&service) {
            Some(image) => crate::contract::describe(image).await,
            None => None,
        };
        let from = running_version(dir, &service).await;
        let stop = said.as_ref().and_then(|said| {
            let oldest = said.upgrade_from.as_deref()?;
            let from = from.as_deref()?;
            (!crate::contract::at_least(from, oldest)).then(|| {
                format!(
                    "`{service}` runs {from}, and the release it would move to can only be \
                     moved to from {oldest} or newer: it has to stop at a release in \
                     between first. It was left on {from}."
                )
            })
        });
        let beside = said.as_ref().and_then(|said| {
            crate::contract::unmet(said, &reached)
                .map(|why| format!("`{service}` was not updated: {why}."))
        });
        match stop.or(beside) {
            Some(reason) => {
                on_event(UpdateEvent::Refused {
                    service: service.clone(),
                    reason: reason.clone(),
                });
                report.refused.push((service, reason));
            }
            None => allowed.push((service, companions)),
        }
    }
    let moving = allowed;

    // --- the builds --------------------------------------------------------------------
    // What moves is pinned to what its channel resolves to now. Whatever else has no build
    // written down yet — a hub from before builds were, a service just added — is pinned to
    // the one it runs, so that nothing but this update's own services changes under it.
    let fetched: Vec<(String, String)> = moving
        .iter()
        .flat_map(|(service, companions)| std::iter::once(service).chain(companions))
        .filter_map(|name| Some((name.clone(), channels.get(name)?.clone())))
        .collect();
    let builds_before = lock::read(dir).pins;
    let mut builds = crate::pins::resolve(&fetched).await;
    let untouched: Vec<(String, String)> = crate::pins::unpinned(&config, &lock::read(dir).pins)
        .into_iter()
        .filter(|(service, _)| !builds.contains_key(service))
        .filter(|(service, _)| !fetched.iter().any(|(name, _)| name == service))
        // A refused service's tag has just been fetched to an image it must not run; with
        // no container to read the build off, it stays unpinned rather than pinned to that.
        .filter(|(service, _)| !report.refused.iter().any(|(name, _)| name == service))
        .collect();
    builds.extend(crate::pins::adopt(dir, &untouched).await);
    if let Err(error) = crate::pins::record(dir, builds) {
        warn(format!(
            "could not write down the builds this hub runs ({error}) — its compose file \
             keeps naming tags"
        ));
    }

    // --- the files -------------------------------------------------------------------
    let read = |name: &str| std::fs::read(dir.join(name)).ok();
    let configs_before = crate::services::snapshot_configs(dir);
    let compose_before = read(crate::compose_file::COMPOSE_FILENAME);
    if let Err(error) = crate::profile::rewrite(dir, config.clone(), &[]) {
        put_back("The files could not be generated");
        return Err(error.into());
    }
    // Each service whose image has a contract writes its own config, from the hub's facts
    // and what the operator set. A release that says it cannot be configured for this hub
    // stops the update here, with nothing replaced.
    let identity = crate::credentials::read_credentials(dir)
        .map(|credentials| credentials.issued_identity())
        .unwrap_or_default();
    let self_written = match crate::contract::render_hub(dir, &config, &identity).await {
        Ok(services) => services,
        Err(error) => {
            put_back("A service would not write its config for this hub");
            return Err(error.into());
        }
    };
    let changed =
        crate::services::changed_configs(&configs_before, &crate::services::snapshot_configs(dir));
    let compose_changed = compose_before != read(crate::compose_file::COMPOSE_FILENAME);
    report.migrated = pending.clone();
    report.rewritten = changed
        .iter()
        .map(|name| format!("configs/{name}"))
        .collect();
    if compose_changed {
        report
            .rewritten
            .push(crate::compose_file::COMPOSE_FILENAME.to_string());
    }
    if !report.rewritten.is_empty() {
        step(format!("Rewrote {}", report.rewritten.join(", ")));
    }

    // --- does each release read what was written for it? ------------------------------
    // The last question before anything is replaced, and the one the files cannot answer
    // themselves: a config a release does not read starts it all the same, with defaults.
    for (service, _) in &moving {
        let is_service = config
            .enabled_services()
            .into_iter()
            .any(|id| config.service(id).host == *service);
        // A config its own image wrote was judged by it as it was written.
        if !is_service || self_written.contains(service) {
            continue;
        }
        match reads_its_config(dir, service).await {
            Reads::Yes => on_event(UpdateEvent::Line {
                line: format!("{service} reads its config as written"),
                stderr: false,
            }),
            Reads::No(said) => {
                put_back(&format!(
                    "`{service}`'s new release does not read the config written for it"
                ));
                return Err(UpdateError::Compose(format!(
                    "`{service}` was not updated: the release it would move to does not read \
                     the config this Konstruktor writes for it, and would start on defaults. \
                     It said:\n{said}\nNothing was replaced. A newer Konstruktor may know \
                     this release."
                )));
            }
            Reads::Failed(said) => warn(format!(
                "`{service}`'s new release could not be asked whether it reads its config; \
                 it is updated all the same. It said:\n{said}"
            )),
            Reads::Unasked => {}
        }
    }

    // --- what the move needs run first ---------------------------------------------------
    // The hub still runs as it did; a failure here leaves it so.
    for moved in &pending {
        for action in moved.at(When::Before) {
            if let Err(error) = run_action(action).await {
                put_back(&format!("\"{}\" failed", action.title));
                return Err(UpdateError::Migration(format!(
                    "the update was stopped before anything was replaced: \"{}\", which \
                     moving this hub's files needs first ({}), failed: {error}",
                    action.title, moved.title
                )));
            }
        }
    }

    // Asked before anything is stopped or recreated: afterwards the answer is always yes.
    let was_running = running(dir).await;

    // --- migrations, and what each release has to do to its own data ---------------------
    // A service whose build changes has its database brought to the new release *before*
    // its container is replaced, as a step with an answer — not inside the new container's
    // start, where a migration that fails is a crash loop somebody notices later. After the
    // schema, whatever the release itself has to do to its data between the two versions,
    // which only it knows (`manage.py upgrade`).
    //
    // Neither may run while the old code still writes, so everything that writes that
    // service's data is stopped first — the service and what moves with it — and all of it
    // happens before a single container is replaced: a failure then still has a hub to go
    // back to, old builds on old files, started again as it was.
    let builds_now = lock::read(dir).pins;
    let is_service = |name: &str| {
        config
            .enabled_services()
            .into_iter()
            .any(|id| config.service(id).host == name)
    };
    let mut preparing: Vec<(String, Option<(String, String)>)> = Vec::new();
    for (service, _) in &moving {
        // The same build as before has nothing to migrate, and is not stopped for it.
        let same_build = builds_before
            .get(service)
            .zip(builds_now.get(service))
            .is_some_and(|(before, now)| before == now);
        if !is_service(service) || same_build {
            continue;
        }
        let reached = match channels.get(service) {
            Some(image) => crate::docker::image_label(image, VERSION_LABEL).await,
            None => None,
        };
        let versions = match (running_version(dir, service).await, reached) {
            // Asked while the old container still serves.
            (Some(from), Some(to)) if from != to && ships_an_upgrade(dir, service).await => {
                Some((from, to))
            }
            _ => None,
        };
        preparing.push((service.clone(), versions));
    }
    if !preparing.is_empty() {
        let is_up = |name: String| async move {
            crate::engine_probe::engine()
                .async_command()
                .args(["compose", "ps", "--status", "running", "-q", &name])
                .current_dir(dir)
                .stdin(std::process::Stdio::null())
                .output()
                .await
                .is_ok_and(|out| !String::from_utf8_lossy(&out.stdout).trim().is_empty())
        };
        let mut stopped: Vec<String> = Vec::new();
        for (service, _) in &preparing {
            let writers = moving
                .iter()
                .find(|(name, _)| name == service)
                .map(|(_, companions)| companions.clone())
                .unwrap_or_default();
            for name in std::iter::once(service.clone()).chain(writers) {
                if is_up(name.clone()).await && !stopped.contains(&name) {
                    stopped.push(name);
                }
            }
        }
        // Both talk to the database, which a stopped hub does not run.
        let database_was_up = is_up(DB_COMPOSE_SERVICE.to_string()).await;
        let compose = |verb: &str, names: &[String]| {
            let mut argv = vec!["compose".to_string(), verb.to_string()];
            argv.extend(names.iter().cloned());
            argv
        };
        // Back to how it was: the files and builds of before, and what was stopped here
        // running again on them.
        let undo = |why: String, stopped: Vec<String>| async move {
            put_back(&why);
            if !stopped.is_empty() {
                let _ = crate::compose::run_streamed(dir, compose("start", &stopped), &line).await;
            }
            if !database_was_up {
                let database = [DB_COMPOSE_SERVICE.to_string()];
                let _ = crate::compose::run_streamed(dir, compose("stop", &database), &line).await;
            }
        };
        if !stopped.is_empty() {
            step(format!("Stopping {} to migrate", stopped.join(", ")));
            if let Err(error) =
                crate::compose::run_streamed(dir, compose("stop", &stopped), &line).await
            {
                undo("A service could not be stopped".into(), stopped).await;
                return Err(UpdateError::Compose(error));
            }
        }
        if !database_was_up {
            let up = vec![
                "compose".to_string(),
                "up".to_string(),
                "-d".to_string(),
                "--no-deps".to_string(),
                DB_COMPOSE_SERVICE.to_string(),
            ];
            let started = crate::compose::run_streamed(dir, up, &line).await;
            let ready = match started {
                Ok(_) => crate::backup::wait_for_database(dir, &config, "database", &|_| {})
                    .await
                    .map_err(|error| error.to_string()),
                Err(error) => Err(error),
            };
            if let Err(error) = ready {
                undo("The database could not be started".into(), stopped).await;
                return Err(UpdateError::Compose(error));
            }
        }
        let undone = "This hub runs the builds it ran, on the files it had. What was done to \
                      the database before the failure is not undone — a migration is applied \
                      whole or not at all, the ones before it stay — and the data as it was \
                      is in the backup.";
        for (service, versions) in &preparing {
            step(format!("Migrating {service}'s database"));
            if let Err(said) = crate::compose::run_streamed(dir, migrate(service), &line).await {
                undo(
                    format!("`{service}`'s database could not be migrated"),
                    stopped,
                )
                .await;
                return Err(UpdateError::Migration(format!(
                    "the update was stopped before anything was replaced: the database of \
                     `{service}` could not be migrated to its new release. It said:\n{}\n{undone}",
                    last_lines(&said)
                )));
            }
            let Some((from, to)) = versions else {
                continue;
            };
            step(format!("{service} upgrades itself from {from} to {to}"));
            if let Upgraded::Failed(said) = upgrade(dir, service, from, to).await {
                undo(
                    format!("`{service}` could not upgrade itself from {from} to {to}"),
                    stopped,
                )
                .await;
                return Err(UpdateError::Migration(format!(
                    "the update was stopped before anything was replaced: `{service}` \
                     could not upgrade itself from {from} to {to}. It said:\n{said}\n{undone}"
                )));
            }
        }
        if !database_was_up {
            let database = [DB_COMPOSE_SERVICE.to_string()];
            let _ = crate::compose::run_streamed(dir, compose("stop", &database), &line).await;
        }
    }

    // --- recreate --------------------------------------------------------------------
    // From here there is no going back to the old files: what the move still has to run
    // afterwards is owed until it has.
    crate::migrate::begin(dir, &pending).map_err(crate::profile::ProfileError::Io)?;
    let mut recreated: Vec<String> = Vec::new();
    for (service, companions) in &moving {
        step(format!("Updating {service}"));
        // `--no-deps`: updating one service on a stopped stack must not boot the rest.
        crate::compose::run_streamed(dir, crate::compose::up_service(service), &line)
            .await
            .map_err(UpdateError::Compose)?;
        // After Rekuest, which migrates the schema takt waits for; `--no-deps` would
        // otherwise leave takt on the old image.
        for companion in companions
            .iter()
            .filter(|c| crate::compose_file::declares_service(dir, c))
        {
            crate::compose::run_streamed(dir, crate::compose::up_service(companion), &line)
                .await
                .map_err(UpdateError::Compose)?;
        }
        on_event(UpdateEvent::Updated {
            service: service.clone(),
        });
        report.updated.push(service.clone());
        recreated.push(service.clone());
        recreated.extend(companions.iter().cloned());
    }

    // --- everything else that reads a file that changed -------------------------------
    // A running hub only: a stopped one reads its files when it is started. `up` creates
    // what the files gained and removes what they lost; a container whose mounted config
    // changed under it is restarted, unless it was just recreated.
    if !report.rewritten.is_empty() && was_running {
        step("Bringing the rest of the hub to the rewritten files".into());
        let restart: Vec<String> = crate::services::services_to_restart(
            &config,
            &changed,
            &crate::services::ServicePlan::default(),
        )
        .into_iter()
        .filter(|name| !recreated.contains(name))
        .collect();
        Box::pin(crate::services::apply_services(dir, &restart, &line))
            .await
            .map_err(|error| UpdateError::Compose(error.to_string()))?;
    }

    // --- what the move needs run last ----------------------------------------------------
    // The hub runs on the new files now. A failure is said and owed, not undone.
    'closing: for moved in &pending {
        for action in moved.at(When::After) {
            if let Err(error) = run_action(action).await {
                warn(format!(
                    "\"{}\" failed ({error}). This hub runs on its new files, and that still \
                     has to run: `konstruktor update` runs it again before anything else.",
                    action.title
                ));
                report.unfinished = crate::migrate::unfinished(dir);
                break 'closing;
            }
        }
        crate::migrate::finish(dir, moved.to).map_err(crate::profile::ProfileError::Io)?;
    }

    if !report.updated.is_empty() || !report.rewritten.is_empty() {
        let _ = lock::record(dir, &config, "updated", lock::now()).await;
    }

    // --- did it come back? ---------------------------------------------------------
    // A migration that fails leaves a container restarting, and `compose up` reports
    // success regardless: it started the container, which is all it claims.
    if request.health_check && !report.updated.is_empty() {
        step("Checking the services still answer".into());
        let health = crate::health::check(dir, &config, &|event| {
            if let crate::health::HealthEvent::Line { line } = event {
                on_event(UpdateEvent::Line {
                    line,
                    stderr: false,
                });
            }
        })
        .await
        .map_err(UpdateError::Health)?;
        report.health = Some(health);
    }

    Ok(report)
}

async fn check_one(local: ImageState) -> UpstreamCheck {
    let base = |state, remote_digest, error| UpstreamCheck {
        service: local.service.clone(),
        image: local.image.clone(),
        state,
        remote_digest,
        error,
    };

    if !local.present {
        return base(UpstreamState::Missing, None, None);
    }

    // A reference that names its own digest cannot move: `repo:tag@sha256:…` resolves to
    // that manifest whatever the tag now points at. So the comparison is against the pin
    // rather than against the registry's answer for the tag — otherwise a pinned image
    // whose channel had moved on would report `Newer` forever, and every update would pull
    // and recreate it to arrive at exactly the image it already had.
    if let Some(pinned) = pinned_digest(&local.image) {
        let held = local.repo_digests.iter().any(|d| digest_of(d) == pinned);
        return base(
            if held {
                UpstreamState::Current
            } else {
                UpstreamState::Newer
            },
            Some(pinned.to_string()),
            None,
        );
    }

    match remote_digest(&local.image).await {
        Ok(remote) => {
            let known = local.repo_digests.iter().any(|d| digest_of(d) == remote);
            if local.repo_digests.is_empty() {
                // Built locally, or loaded from a tarball: there is no digest to compare.
                base(
                    UpstreamState::Unknown,
                    Some(remote),
                    Some("local image carries no registry digest".to_string()),
                )
            } else if known {
                base(UpstreamState::Current, Some(remote), None)
            } else {
                base(UpstreamState::Newer, Some(remote), None)
            }
        }
        Err(error) => base(UpstreamState::Unknown, None, Some(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refusal, or a service that no longer answers, is not a successful update — even
    /// though compose recreated everything it was asked to.
    #[test]
    fn an_update_succeeds_only_when_everything_moved_and_answers() {
        let health = |healthy| crate::health::ServiceHealth {
            service: "mikro".into(),
            container_state: Some("running".into()),
            restarts_seen: false,
            http_status: Some(200),
            url: None,
            healthy,
            detail: String::new(),
        };
        let updated = UpdateReport {
            updated: vec!["mikro".into()],
            health: Some(vec![health(true)]),
            ..Default::default()
        };
        assert!(updated.succeeded());

        let sick = UpdateReport {
            health: Some(vec![health(false)]),
            ..updated.clone()
        };
        assert!(!sick.succeeded());

        let refused = UpdateReport {
            refused: vec![("db".into(), "a new major".into())],
            ..updated.clone()
        };
        assert!(!refused.succeeded());

        // Unchecked is not unhealthy.
        let unchecked = UpdateReport {
            health: None,
            ..updated
        };
        assert!(unchecked.succeeded());
    }

    /// The reporter follows `latest` and holds no data; it moves with the services.
    #[test]
    fn the_reporter_is_not_held_back_as_infrastructure() {
        let mut config = crate::config::hub::build_hub_config(&Default::default());
        config.reporter = Some(crate::config::hub::ReporterBlock::default());
        assert!(!is_infrastructure(&config, "reporter"));
        assert!(is_infrastructure(&config, "db"));
        assert!(!is_infrastructure(&config, "mikro"));
    }

    #[test]
    fn hub_references() {
        assert_eq!(
            parse("jhnnsrs/rekuest:next"),
            Reference {
                host: "registry-1.docker.io".into(),
                repository: "jhnnsrs/rekuest".into(),
                tag: "next".into()
            }
        );
        assert_eq!(parse("postgres").repository, "library/postgres");
        assert_eq!(parse("postgres").tag, "latest");
        assert_eq!(
            parse("docker.io/library/redis:7").host,
            "registry-1.docker.io"
        );
    }

    #[test]
    fn other_registries() {
        let ghcr = parse("ghcr.io/arkitektio/kabinet:dev");
        assert_eq!(ghcr.host, "ghcr.io");
        assert_eq!(ghcr.repository, "arkitektio/kabinet");
        let ported = parse("localhost:5000/thing");
        assert_eq!(ported.host, "localhost:5000");
        assert_eq!(ported.repository, "thing");
        assert_eq!(ported.tag, "latest");
        assert_eq!(parse("quay.io/minio/minio@sha256:abc").tag, "latest");
    }

    /// The rule that keeps an advance from becoming a migration. A hub on `16.13-1` may be
    /// offered `16.14-1`; it may not be offered `17.0-1`, whatever the registry publishes,
    /// because Postgres will not open the old cluster and no update should propose that
    /// silently. Variants have to match too, or a plain image would be offered an alpine
    /// one.
    #[test]
    fn only_a_newer_version_of_the_same_major_and_variant_is_offered() {
        let available: Vec<String> = [
            "dev",
            "latest",
            "16.13-1",
            "16.14-1",
            "16.14-1-alpine",
            "17.0-1",
            "16.9-1",
        ]
        .iter()
        .map(|t| t.to_string())
        .collect();

        assert_eq!(newer_version_tag("16.13-1", &available), Some("16.14-1"));
        // Already on the newest of its major: nothing to offer, and 17 is not an offer.
        assert_eq!(newer_version_tag("16.14-1", &available), None);
        // An alpine hub stays on alpine.
        assert_eq!(
            newer_version_tag("16.13-1-alpine", &available),
            Some("16.14-1-alpine")
        );
        // A channel is not a version, so there is nothing to order it against.
        assert_eq!(newer_version_tag("dev", &available), None);
        // Nor is MinIO's release stamp — and being unable to say so is the right answer.
        assert_eq!(
            newer_version_tag("RELEASE.2025-02-18T16-25-55Z", &available),
            None
        );
    }

    #[test]
    fn a_version_is_the_numbers_a_tag_starts_with() {
        assert_eq!(
            version_of("16.13-1").expect("a version").parts,
            vec![16, 13, 1]
        );
        assert_eq!(
            version_of("8.11.0-alpine").expect("a version").variant,
            "alpine"
        );
        assert!(version_of("dev").is_none());
        assert!(version_of("RELEASE.2025-02-18T16-25-55Z").is_none());
    }

    /// The split has to come from the profile rather than a list of names, so that a
    /// service added upstream is classified without this being edited.
    #[test]
    fn services_are_not_infrastructure_and_everything_else_is() {
        use crate::config::hub::{build_hub_config, HubConfigOptions};
        let config = build_hub_config(&HubConfigOptions {
            device_id: "device".into(),
            coord_server: "go.arkitekt.live".into(),
            ..Default::default()
        });

        for id in config.enabled_services() {
            let host = config.service(id).host.clone();
            assert!(
                !is_infrastructure(&config, &host),
                "{host} is one of the hub's own services"
            );
        }
        for infra in ["db", "redis", "rustfs", "rustfs_init", "gateway"] {
            assert!(
                is_infrastructure(&config, infra),
                "{infra} is infrastructure"
            );
        }
    }

    /// Everything but the database is waved through, and the database is never waved
    /// through on a version nobody could read — an unverifiable major is a warning, not a
    /// pass.
    #[tokio::test]
    async fn only_the_database_is_guarded_and_never_silently() {
        use crate::config::hub::{build_hub_config, HubConfigOptions};
        let mut config = build_hub_config(&HubConfigOptions {
            device_id: "device".into(),
            coord_server: "go.arkitekt.live".into(),
            ..Default::default()
        });
        let dir = std::env::temp_dir().join(format!("konstruktor-guard-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).expect("a scratch folder");

        assert_eq!(guard(&dir, &config, "rekuest").await, Guard::Clear);
        assert_eq!(guard(&dir, &config, "gateway").await, Guard::Clear);

        // A folder-mode database with no cluster on disk yet: nothing to read, so nothing
        // to claim.
        config.db.mount = Some("./db_data".into());
        assert!(
            matches!(guard(&dir, &config, "db").await, Guard::Warn(_)),
            "an unreadable major must not report as clear"
        );
    }

    #[test]
    fn a_digest_pin_is_read_off_the_reference() {
        assert_eq!(
            pinned_digest("jhnnsrs/daten:dev@sha256:abc"),
            Some("sha256:abc")
        );
        assert_eq!(pinned_digest("caddy:2.11.4"), None);
    }

    #[test]
    fn digest_strips_repo() {
        assert_eq!(digest_of("jhnnsrs/rekuest@sha256:abc"), "sha256:abc");
    }

    /// Only `validate_settings --strict`'s own exit code refuses. An image that does not
    /// know the flag was never asked, and a crash is said without stopping the update.
    #[test]
    fn only_the_commands_own_no_is_a_refusal() {
        assert_eq!(reads_from(Some(0), "Configuration valid"), Reads::Yes);
        assert_eq!(
            reads_from(Some(78), "tree\n\nnot read: rekuest.service_agents\n"),
            Reads::No("tree\nnot read: rekuest.service_agents".into())
        );
        assert_eq!(
            reads_from(Some(2), "error: unrecognized arguments: --strict"),
            Reads::Unasked
        );
        assert_eq!(reads_from(Some(127), "python: not found"), Reads::Unasked);
        assert_eq!(reads_from(None, ""), Reads::Unasked);
        assert_eq!(
            reads_from(Some(1), "Traceback\nImportError: no module"),
            Reads::Failed("Traceback\nImportError: no module".into())
        );
    }

    /// A release with no `upgrade` command has nothing of its own to do; one whose
    /// upgrade exits anything but 0 has failed.
    #[test]
    fn an_upgrade_that_is_not_there_is_not_a_failure() {
        assert_eq!(upgraded_from(Some(0), "nothing to do"), Upgraded::Done);
        assert_eq!(
            upgraded_from(Some(2), "Unknown command: 'upgrade'"),
            Upgraded::None
        );
        assert_eq!(upgraded_from(Some(127), ""), Upgraded::None);
        assert_eq!(
            upgraded_from(Some(1), "Traceback\nValueError: bad row"),
            Upgraded::Failed("Traceback\nValueError: bad row".into())
        );
        assert_eq!(
            upgraded_from(None, "killed"),
            Upgraded::Failed("killed".into())
        );
    }

    /// The tag may be current on this machine while the hub still runs an older build of
    /// it: what counts is the build written down.
    #[test]
    fn a_hub_pinned_to_an_older_build_than_the_registry_serves_has_an_update() {
        let pins = std::collections::BTreeMap::from([(
            "mikro".to_string(),
            crate::lock::Pin {
                image: "jhnnsrs/mikro:5".into(),
                digest: Some("sha256:runs".into()),
            },
        )]);
        let check = |service: &str, remote: &str| UpstreamCheck {
            service: service.into(),
            image: format!("jhnnsrs/{service}:5"),
            state: UpstreamState::Current,
            remote_digest: Some(remote.into()),
            error: None,
        };
        let mut behind = check("mikro", "sha256:newer");
        behind_its_pin(&mut behind, &pins);
        assert_eq!(behind.state, UpstreamState::Newer);

        let mut current = check("mikro", "sha256:runs");
        behind_its_pin(&mut current, &pins);
        assert_eq!(current.state, UpstreamState::Current);

        // No build written down for it: the tag's answer stands.
        let mut unpinned = check("fluss", "sha256:newer");
        behind_its_pin(&mut unpinned, &pins);
        assert_eq!(unpinned.state, UpstreamState::Current);
    }

    /// The paths that differ are named, never what is at them: a config holds secrets.
    #[test]
    fn a_config_preview_names_keys_and_shows_no_values() {
        let before: serde_norway::Value = serde_norway::from_str(
            "django: {secret_key: aaa, debug: false}\nrekuest: {service_agents: [1]}\nsame: 1\n",
        )
        .unwrap();
        let after: serde_norway::Value = serde_norway::from_str(
            "django: {secret_key: bbb, debug: false}\nrekuest: {services: [1], hook_agents: []}\nsame: 1\n",
        )
        .unwrap();
        assert_eq!(
            changed_keys(&before, &after),
            [
                "django.secret_key",
                "rekuest.hook_agents (new)",
                "rekuest.service_agents (gone)",
                "rekuest.services (new)"
            ]
        );
        assert_eq!(changed_keys(&before, &before), Vec::<String>::new());
    }

    #[test]
    fn the_migrations_a_release_would_apply_are_read_off_its_plan() {
        let plan = "17:07:55 INFO embeddings.engine: loaded\nPlanned operations:\nfacade.0011_drop_lease_epoch\n    Remove field lease_epoch from task\nauthentikate.0007_user_claims\n    Add field x to user\n";
        assert_eq!(
            planned_migrations(plan),
            [
                "facade.0011_drop_lease_epoch",
                "authentikate.0007_user_claims"
            ]
        );
        assert_eq!(
            planned_migrations("Planned operations:\n  No planned migration operations.\n"),
            Vec::<String>::new()
        );
    }
}
