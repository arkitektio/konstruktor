//! What a service's image says of itself, and its config written by it.
//!
//! A hub's installer knows the hub: where the database is, which services run, which keys
//! they trust. What a *service* is — what it needs, how this release of it spells its
//! config — the service's own image says, through one entry point every image has
//! (the `arkitekt-service` package; run with no command, an image says what it is):
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
pub const CONTRACT: u32 = 2;

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
///
/// A closed block, as it is on the service's side: a key this build does not know is not
/// skipped over. An image from before databases had names says `database`, and reading
/// that as "nothing said, so `main`" would provide it something it never asked for by
/// that name — it is an image this build does not run, like any that speaks another
/// contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Needs {
    /// A database for each of these names, in the hub's Postgres. The hub calls each
    /// `<service>_<name>` ([`crate::config::hub::database_name`]); the service is handed
    /// them by the name it gave. Left unsaid, one called `main`; empty for a service
    /// that keeps nothing in Postgres.
    #[serde(default = "main_database")]
    pub databases: Vec<String>,
    /// The hub's Redis. Left unsaid, it needs it.
    #[serde(default = "yes")]
    pub redis: bool,
    /// The buckets it stores into, by purpose (`media`, `zarr`): one bucket each, and the
    /// credentials to hand out grants for them. A purpose is the service's own word; the
    /// hub only has to provide a bucket for it.
    #[serde(default)]
    pub storage: Vec<String>,
    /// What a token may be allowed to do at the service, defined at the coordination
    /// server when the hub enrols.
    #[serde(default)]
    pub scopes: Vec<Scope>,
    /// The roles a member of an organization can hold at the service.
    #[serde(default)]
    pub roles: Vec<Scope>,
    /// A key of its own, to sign what it sends the hub's other services with.
    #[serde(default)]
    pub instance_key: bool,
    /// The hub's operator account. Left unsaid, it is told of it.
    #[serde(default = "yes")]
    pub admin: bool,
    #[serde(default)]
    pub peers: Vec<String>,
    /// Secrets the hub mints for it once and keeps, by name: each is handed to the service
    /// as a file only it can read.
    #[serde(default)]
    pub secrets: Vec<String>,
}

fn yes() -> bool {
    true
}

fn main_database() -> Vec<String> {
    vec![crate::config::hub::MAIN_DATABASE.to_string()]
}

impl Default for Needs {
    /// What an image that says nothing of its needs is taken to need: the same a field
    /// left out of a description reads as.
    fn default() -> Self {
        Self {
            databases: main_database(),
            redis: true,
            storage: Vec::new(),
            scopes: Vec::new(),
            roles: Vec::new(),
            instance_key: false,
            admin: true,
            peers: Vec::new(),
            secrets: Vec::new(),
        }
    }
}

/// The name Rekuest goes by among a service's peers, and the kind of endpoint a service
/// offers it: see [`Description::hooked_by_rekuest`].
pub const REKUEST_PEER: &str = "rekuest";
pub const REKUEST_HOOK: &str = "rekuest_hook";

/// What a service offers a hub.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offers {
    #[serde(default)]
    pub health: String,
    /// Endpoints other services are wired to, by kind, as paths under the service's own.
    #[serde(default)]
    pub endpoints: BTreeMap<String, String>,
}

/// Where the code in an image came from, and where it sits in it: what it takes to run the
/// service from a checkout instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// What to clone: `https://github.com/arkitektio/mikro-server-next`.
    pub repository: String,
    /// The commit the image was built from, when the build said.
    #[serde(default)]
    pub revision: Option<String>,
    /// Where the service's code sits in the image: a checkout is mounted here.
    #[serde(default = "workspace")]
    pub path: String,
}

/// Where a service's code sits in its image unless the image says otherwise.
pub const WORKSPACE: &str = "/workspace";

fn workspace() -> String {
    WORKSPACE.to_string()
}

/// One descriptor of a structure's objects (`@mikro/n_channels`, an `INT`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Descriptor {
    pub key: String,
    /// What its value is, in the service's own word for it; `ANY` when it does not say.
    #[serde(rename = "type", default = "any")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

fn any() -> String {
    "ANY".to_string()
}

/// A kind of object a service holds, known across the hub by its identifier
/// (`@mikro/arraydataset`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Structure {
    pub identifier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub descriptors: Vec<Descriptor>,
}

/// What a service announces about a structure's objects, and with which descriptors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signal {
    pub identifier: String,
    /// `CREATED`, `UPDATED`, `DELETED`: which of them it announces.
    #[serde(default = "created")]
    pub kinds: Vec<String>,
    /// The descriptor keys an announcement carries.
    #[serde(default)]
    pub descriptors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

fn created() -> Vec<String> {
    vec!["CREATED".to_string()]
}

/// What exists on a hub because a service is there: the structures it holds and the
/// signals it sends about them.
///
/// Said by the image, so that a hub knows it without asking the running service. Nothing
/// here acts on it: it is handed, as it was said, to the service of the hub that keeps
/// the catalogue of what there is ([`facts`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hosts {
    #[serde(default)]
    pub structures: Vec<Structure>,
    #[serde(default)]
    pub signals: Vec<Signal>,
}

impl Hosts {
    pub fn is_empty(&self) -> bool {
        self.structures.is_empty() && self.signals.is_empty()
    }
}

/// The job a service offers for taking in what the other services of the hub host
/// ([`Hosts`]): the one that declares it is the one that is told.
pub const CATALOGUE_JOB: &str = "catalogue";

/// Something that can be run in a service's image, as a container of its own, by name. The
/// start is not one of them: that is the image's own command, and nothing here writes one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    /// What to run, in a container of the image, with the service's config.
    pub command: Vec<String>,
    #[serde(default)]
    pub summary: String,
    /// Other jobs this one runs as part of itself, in order: `migrate` includes the
    /// service's setup.
    #[serde(default)]
    pub includes: Vec<String>,
}

/// A process that runs beside a service, as an image of its own: one the service does not
/// run without (Rekuest's takt), or an optional one it drives on a hub that has the use for
/// it (Lok's mesh control server).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sidecar {
    /// What it runs as beside the service: `takt` is `<service>-takt`.
    pub name: String,
    /// Its image, from the service's own: `{repository}` and `{tag}` stand for the parts of
    /// the image the description came from.
    pub image: String,
    #[serde(default)]
    pub summary: String,
    /// Whether the service runs without it: started only on a hub that asked for what it
    /// brings.
    #[serde(default)]
    pub optional: bool,
}

/// What the images of a hub say of themselves, by compose service: what its files are
/// written from, beside its profile.
pub type Said = BTreeMap<String, Description>;

impl Description {
    /// The command a container of the service runs: `serve`, or `debug` when asked for.
    /// `None` leaves it to the image's own.
    pub fn command(&self, debug: bool) -> Option<&[String]> {
        let command = if debug { &self.debug } else { &self.serve };
        (!command.is_empty()).then_some(command.as_slice())
    }

    /// Whether Rekuest runs this service's periodic actions and receives its signals, each
    /// call signed with the sender's instance key: it names Rekuest among its peers, or
    /// offers the endpoint Rekuest calls. A hub cannot take its Rekuest out while a service
    /// that says so runs.
    pub fn hooked_by_rekuest(&self) -> bool {
        self.needs.peers.iter().any(|peer| peer == REKUEST_PEER)
            || self.offers.endpoints.contains_key(REKUEST_HOOK)
    }

    /// Where the service answers for its health, under its own path: what it offers, or
    /// the convention ([`crate::health::HEALTH_PATH`]) when it names none.
    pub fn health_path(&self) -> &str {
        let offered = self.offers.health.trim().trim_matches('/');
        if offered.is_empty() {
            crate::health::HEALTH_PATH
        } else {
            offered
        }
    }

    /// Whether the service keeps the hub's catalogue of what its services host: it offers
    /// the job that takes that in ([`CATALOGUE_JOB`]). Such a service is told what every
    /// other one hosts; one that offers no such job has no use for it, and a release from
    /// before services said what they host would refuse facts that mention it.
    pub fn catalogues(&self) -> bool {
        self.jobs.contains_key(CATALOGUE_JOB)
    }

    /// The command of the job that prepares the service, if it has one to run.
    pub fn preparation(&self) -> Option<&[String]> {
        let job = self.jobs.get(self.prepare.as_deref()?)?;
        Some(&job.command)
    }
}

impl Sidecar {
    /// The image this sidecar runs, beside a service running `service_image`.
    pub fn image_beside(&self, service_image: &str) -> String {
        let reference = service_image.split('@').next().unwrap_or(service_image);
        // A colon after the last slash separates the tag; one before it is a registry's port.
        let name_starts = reference.rfind('/').map_or(0, |slash| slash + 1);
        let (repository, tag) = match reference[name_starts..].rfind(':') {
            Some(colon) => (
                &reference[..name_starts + colon],
                &reference[name_starts + colon + 1..],
            ),
            None => (reference, "latest"),
        };
        self.image
            .replace("{repository}", repository)
            .replace("{tag}", tag)
    }
}

/// A service, as its image describes it (`arkitekt_service.contract.description.Description`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Description {
    pub contract: u32,
    pub name: String,
    /// What it is, in a line.
    #[serde(default)]
    pub summary: String,
    /// What the service is registered as at the coordination server, and what a client asks
    /// for (`live.arkitekt.mikro`).
    pub identifier: String,
    /// What writes the release's config: run in the image with the hub's facts mounted.
    #[serde(default)]
    pub render: Vec<String>,
    /// What a container of the image runs to serve, and nothing else: written as the
    /// service's command.
    #[serde(default)]
    pub serve: Vec<String>,
    /// The same for development (`--debug`): the server that reloads on a change.
    #[serde(default)]
    pub debug: Vec<String>,
    /// What can be run in the image beside its start, by name.
    #[serde(default)]
    pub jobs: BTreeMap<String, Job>,
    /// The job that brings the service's database to this release, run once per build
    /// before it is started. `None` when there is nothing to prepare.
    #[serde(default)]
    pub prepare: Option<String>,
    #[serde(default)]
    pub sidecars: Vec<Sidecar>,
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
    /// The structures the service holds and the signals it sends about them.
    #[serde(default)]
    pub hosts: Hosts,
    /// Where the image's code came from. `None` for an image that does not say: it cannot
    /// be run from a checkout without being told one.
    #[serde(default)]
    pub source: Option<Source>,
}

/// Asks `image` what it is, by running it with no command: a service's image answers with
/// its description and stops. Nothing of what is inside is assumed — not a language, not a
/// module — so anything that prints the description as its own command is a service. `None`
/// for an image that does not: every release before this was the convention, or one that
/// speaks a contract this build does not.
pub async fn describe(image: &str) -> Option<Description> {
    let answer = answer(image).await?;
    serde_json::from_str::<Description>(&answer)
        .ok()
        .filter(|said| said.contract == CONTRACT)
}

/// What `image` printed when it was run with no command, as it printed it: its description,
/// if it is a service's. For showing an operator the whole of it (`konstruktor inspect`);
/// everything that acts on a description reads [`describe`].
pub async fn answer(image: &str) -> Option<String> {
    // Named, so it can be removed: an image that is not a service's may well *start*
    // something with no command, and never stop to answer.
    let name = format!("konstruktor-describe-{:012x}", rand::random::<u64>() >> 16);
    let asking = crate::engine_probe::engine()
        .async_command()
        .args(["run", "--rm", "--name", &name, image])
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let output = match tokio::time::timeout(DESCRIBE_TIMEOUT, asking).await {
        Ok(output) => output.ok().filter(|output| output.status.success())?,
        Err(_) => {
            let _ = crate::engine_probe::engine()
                .async_command()
                .args(["rm", "--force", &name])
                .output()
                .await;
            return None;
        }
    };
    String::from_utf8(output.stdout).ok()
}

/// The image `service` of the hub at `dir` runs: the build written down for it, or what
/// the profile names. The hub's own coordination server counts.
pub fn image_of(dir: &Path, config: &HubConfig, service: &str) -> Option<String> {
    if let Some((_, _, image)) = images(dir, config)
        .into_iter()
        .find(|(_, host, _)| host == service)
    {
        return Some(image);
    }
    let lok = config.running_lok().filter(|lok| lok.host == service)?;
    let pinned = crate::pins::references(config, &crate::lock::read(dir).pins);
    Some(pinned.get(&lok.host).unwrap_or(&lok.image).clone())
}

/// How long an image gets to say what it is, fetching it included. One that describes itself
/// answers in a second; this is for the one that serves something instead.
const DESCRIBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

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
pub async fn render(image: &str, command: &[String], facts: &Path, overrides: &Path) -> Rendered {
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
    // What writes its config is the image's to name (`render` of its description).
    args.push(image.to_string());
    args.extend(command.iter().cloned());
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
        "`{service}` runs {image}, which does not answer the hub contract (run with no \
         command, a service's image says what it is). A service's config is its own image's to write, so \
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
    let images = images(dir, config);
    // The ones not asked yet on this build, asked side by side: each is a container of
    // its own, and none of them knows of the others.
    let unknown: Vec<&(ServiceId, String, String)> = images
        .iter()
        .filter(|(_, host, image)| {
            held.described
                .get(host)
                .is_none_or(|known| &known.image != image)
        })
        .collect();
    let answers =
        futures_util::future::join_all(unknown.iter().map(|(_, _, image)| describe(image))).await;
    let asked = !unknown.is_empty();
    for ((_, host, image), description) in unknown.into_iter().zip(answers) {
        held.described.insert(
            host.clone(),
            crate::lock::Described {
                image: image.clone(),
                description,
            },
        );
    }
    let out = images
        .iter()
        .filter_map(|(_, host, _)| {
            let said = held.described.get(host)?.description.clone()?;
            Some((host.clone(), said))
        })
        .collect();
    if asked {
        // Re-read: describing takes a while, and the lock may have been written meanwhile.
        let mut now = crate::lock::read(dir);
        now.described = held.described;
        let _ = crate::lock::write(dir, &now);
    }
    out
}

/// Refuses a hub that would run something else beside a service than the service's image
/// says it is released with. How a sidecar is wired into the stack is still written here
/// (takt, beside Rekuest); which build of it runs is the service's to say, and the two
/// drifting apart is a pair that was never tested together.
fn sidecars_agree(
    config: &HubConfig,
    said: &BTreeMap<String, Description>,
) -> Result<(), RenderError> {
    let rekuest = config.rekuest();
    let (Some(description), Some(image), Some(running)) = (
        said.get(&rekuest.host),
        rekuest.image.as_deref(),
        config.takt_image(),
    ) else {
        return Ok(());
    };
    // A sidecar the operator pinned by hand is theirs to answer for.
    if config.takt_image.is_some() {
        return Ok(());
    }
    for sidecar in description.sidecars.iter().filter(|s| s.name == "takt") {
        let declared = sidecar.image_beside(image);
        let strip = |reference: &str| reference.split('@').next().unwrap_or(reference).to_owned();
        if strip(&declared) != strip(&running) {
            return Err(RenderError::Failed {
                service: rekuest.host.clone(),
                said: format!(
                    "its image is released with `{declared}` beside it, but this hub would run `{running}`"
                ),
            });
        }
    }
    Ok(())
}

/// The job that brings `service`'s database to its build, as its image declares it: `None`
/// for an image that does not describe itself, or has nothing to prepare.
pub async fn migrate_job(dir: &Path, config: &HubConfig, service: &str) -> Option<Vec<String>> {
    let said = described(dir, config).await.remove(service)?;
    said.preparation().map(<[String]>::to_vec)
}

/// What `service`'s image says of itself, the hub's own coordination server included: it is
/// not one of the hub's services, but its image answers the same question.
pub async fn description_of(dir: &Path, config: &HubConfig, service: &str) -> Option<Description> {
    if let Some(said) = described(dir, config).await.remove(service) {
        return Some(said);
    }
    let lok = config.running_lok().filter(|lok| lok.host == service)?;
    let pinned = crate::pins::references(config, &crate::lock::read(dir).pins);
    describe(pinned.get(&lok.host).unwrap_or(&lok.image)).await
}

/// The command that runs `job` of `service`, with `extra` passed on to it — in a container of
/// its own, beside whatever of the hub is running (`konstruktor job run`).
pub fn job_command(service: &str, job: &Job, extra: &[String]) -> Vec<String> {
    ["compose", "run", "--rm", "--no-deps", "-T", service]
        .into_iter()
        .map(String::from)
        .chain(job.command.iter().cloned())
        .chain(extra.iter().cloned())
        .collect()
}

/// The services of `config`'s stack that `said` holds no description for, as
/// `` `host` (image) ``: the ones nothing can be written or registered for. The hub's own
/// coordination server counts.
pub fn undescribed(config: &HubConfig, said: &Said) -> Vec<String> {
    config
        .enabled_services()
        .into_iter()
        .map(|id| config.service(id))
        .map(|block| (block.host.clone(), block.image.clone().unwrap_or_default()))
        .chain(
            config
                .running_lok()
                .map(|lok| (lok.host.clone(), lok.image.clone())),
        )
        .filter(|(host, _)| !said.contains_key(host))
        .map(|(host, image)| format!("`{host}` ({image})"))
        .collect()
}

/// Refuses a hub in which an image runs as another service than the one it says it is.
///
/// A service's name is its compose service, its path on the gateway and its database, and
/// its config is written by its image for the service the image knows itself as: an image
/// that says `mikro`, run as `example`, would be configured as one and served as the other.
pub fn names_agree(config: &HubConfig, said: &Said) -> Result<(), String> {
    for id in config.enabled_services() {
        let block = config.service(id);
        let Some(description) = said.get(&block.host) else {
            continue;
        };
        if description.name.trim() != id.as_str() {
            return Err(format!(
                "`{}` would run {}, which says it is `{}` — a service runs under the name \
                 its image gives it.",
                block.host,
                block.image.as_deref().unwrap_or("no image"),
                description.name
            ));
        }
    }
    Ok(())
}

/// Refuses a hub whose images ask for something it cannot give them, before anything of
/// what they said is taken in: an image under another service's name
/// ([`names_agree`]), or a database that cannot be provided
/// ([`HubConfig::databases_can_be_provided`]).
pub fn acceptable(config: &HubConfig, said: &Said) -> Result<(), String> {
    names_agree(config, said)?;
    config
        .databases_can_be_provided(said)
        .map_err(|error| format!("{error}."))
}

/// What the images of the hub at `dir` said of themselves when they were last asked: read
/// back from what was written down, without asking anything. What its files are regenerated
/// from.
pub fn known(dir: &Path) -> Said {
    crate::lock::read(dir)
        .described
        .into_iter()
        .filter_map(|(host, known)| Some((host, known.description?)))
        .collect()
}

/// Asks every image a new hub would run what it is, side by side — before anything of the
/// hub is written, because its files are written from the answers. The coordination server
/// of a hub that runs its own is asked too. An image that does not answer is not in the
/// result: what that means is the caller's to say. `already` holds answers there are, by
/// image.
pub async fn describe_all(config: &HubConfig, already: &BTreeMap<String, Description>) -> Said {
    let mut images: Vec<(String, String)> = config
        .enabled_services()
        .into_iter()
        .filter_map(|id| {
            let block = config.service(id);
            Some((block.host.clone(), block.image.clone()?))
        })
        .collect();
    if let Some(lok) = config.running_lok() {
        images.push((lok.host.clone(), lok.image.clone()));
    }
    // One that was asked already — to learn which service it is — is not asked again.
    let answers = futures_util::future::join_all(images.iter().map(|(_, image)| async move {
        match already.get(image) {
            Some(said) => Some(said.clone()),
            None => describe(image).await,
        }
    }))
    .await;
    images
        .into_iter()
        .zip(answers)
        .filter_map(|((host, _), said)| Some((host, said?)))
        .collect()
}

/// Writes down what a new hub's images said, so its first start does not ask them again.
pub fn remember(dir: &Path, config: &HubConfig, said: &Said) -> std::io::Result<()> {
    let mut held = crate::lock::read(dir);
    for id in config.enabled_services() {
        let block = config.service(id);
        if let (Some(image), Some(description)) = (&block.image, said.get(&block.host)) {
            held.described.insert(
                block.host.clone(),
                crate::lock::Described {
                    image: image.clone(),
                    description: Some(description.clone()),
                },
            );
        }
    }
    if let Some(lok) = config.running_lok() {
        if let Some(description) = said.get(&lok.host) {
            held.described.insert(
                lok.host.clone(),
                crate::lock::Described {
                    image: lok.image.clone(),
                    description: Some(description.clone()),
                },
            );
        }
    }
    crate::lock::write(dir, &held)
}

/// The key a peer's [`Hosts`] are told under, in the facts.
const HOSTS: &str = "hosts";

/// What a config is written from, as three hashes: all of it, what the service is told
/// its peers host, and everything apart from that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marks {
    pub from: String,
    pub hosts: String,
    pub apart: String,
}

/// The [`Marks`] of a config written by `image` from `facts`, with `overrides` laid over.
///
/// The two parts are told apart because they ask different things of a running service.
/// Most of what a service is told it reads when it starts, so a change there restarts
/// it. What its peers host it takes in through a job of its own ([`CATALOGUE_JOB`]),
/// running, and restarting a hub's Rekuest — and with it every agent's connection —
/// because Mikro gained a structure would be a high price for a line in a catalogue.
pub fn marks(image: &str, facts: &Value, overrides: &str) -> Marks {
    let mut apart = facts.clone();
    let mut hosts = serde_norway::Mapping::new();
    if let Some(Value::Mapping(peers)) = apart.get_mut("peers") {
        for (name, peer) in peers.iter_mut() {
            if let Value::Mapping(peer) = peer {
                if let Some(hosted) = peer.remove(HOSTS) {
                    hosts.insert(name.clone(), hosted);
                }
            }
        }
    }
    let hash = |document: &Value| {
        crate::lock::digest(
            format!("{image}\n{}\n{overrides}", crate::generate::dump(document)).as_bytes(),
        )
    };
    Marks {
        from: hash(facts),
        hosts: crate::lock::digest(crate::generate::dump(&Value::Mapping(hosts)).as_bytes()),
        apart: hash(&apart),
    }
}

/// Whether a service whose config is being written again owes a run of its catalogue job,
/// and — `Some(true)` — whether that is all the rewriting asks of it, so that it is not
/// restarted.
///
/// `previous` is what its config was last written from, `owed` what it already owed. A
/// config written for the first time owes nothing: preparing a service's database runs
/// its setup, the catalogue included.
pub fn owes_catalogue(
    previous: Option<&crate::lock::Rendering>,
    marks: &Marks,
    owed: Option<bool>,
) -> Option<bool> {
    let Some(previous) = previous else {
        return owed;
    };
    let nothing_hosted = crate::lock::digest(
        crate::generate::dump(&Value::Mapping(serde_norway::Mapping::new())).as_bytes(),
    );
    let hosts_changed = match previous.hosts.as_deref() {
        Some(before) => before != marks.hosts,
        // Written before what peers host was told at all: a change only if there is
        // something to tell now.
        None => marks.hosts != nothing_hosted,
    };
    let nothing_else = previous.apart.as_deref() == Some(marks.apart.as_str());
    match (hosts_changed, owed) {
        (true, owed) => Some(nothing_else && owed.unwrap_or(true)),
        // It owed a run already, and now something else changed too: it is restarted.
        (false, Some(only)) => Some(only && nothing_else),
        (false, None) => None,
    }
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
    sidecars_agree(config, &said)?;
    let written = |error: std::io::Error| RenderError::Write(error.to_string());
    let mut rendered = Vec::new();
    // What is to be asked of which image: decided for all of them first, so the asking
    // itself — a container each — can happen side by side.
    let mut asking = Vec::new();
    for (id, host, image) in images(dir, config) {
        if !said.contains_key(&host) {
            return Err(RenderError::NoContract {
                service: host,
                image,
            });
        }
        let told = facts(config, id, issued, &said);
        let document = crate::generate::dump(&told);
        let overrides = crate::overrides::path(dir, &host);
        let marks = marks(
            &image,
            &told,
            &std::fs::read_to_string(&overrides).unwrap_or_default(),
        );
        let from = marks.from.clone();
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
        let command = said[&host].render.clone();
        asking.push((host, image, command, facts_file, overrides, target, marks));
    }

    let answers = futures_util::future::join_all(asking.iter().map(
        |(_, image, command, facts_file, overrides, _, _)| {
            render(image, command, facts_file, overrides)
        },
    ))
    .await;

    for ((host, _, _, _, _, target, marks), answer) in asking.into_iter().zip(answers) {
        let text = match answer {
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
        // What its peers host changed: its catalogue job is owed, and run once the hub
        // is up on these files ([`crate::services::catalogue`]).
        let owed = owes_catalogue(
            held.rendered.get(&host),
            &marks,
            held.recatalogue.get(&host).copied(),
        );
        match owed {
            Some(only) => held.recatalogue.insert(host.clone(), only),
            None => held.recatalogue.remove(&host),
        };
        held.rendered.insert(
            host,
            crate::lock::Rendering {
                from: marks.from,
                config: config_digest,
                hosts: Some(marks.hosts),
                apart: Some(marks.apart),
            },
        );
        crate::lock::write(dir, &held).map_err(written)?;
    }
    Ok(rendered)
}

/// The command that brings `service`'s database to the build it is about to run: the job
/// its image declares (`jobs.migrate`), which waits for the database, applies the migrations
/// and runs the service's own setup — in a container of that build, beside nothing else of
/// the service.
pub fn prepare(service: &str, job: &[String]) -> Vec<String> {
    ["compose", "run", "--rm", "--no-deps", "-T", service]
        .into_iter()
        .map(String::from)
        .chain(job.iter().cloned())
        .collect()
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
    // The hub's own coordination server too: its start only serves, like any service's.
    // It is not one of the hub's services (nothing is rendered for it), but its database
    // is brought to its build by the same job.
    let lok = config.running_lok().map(|lok| {
        let pinned = crate::pins::references(config, &crate::lock::read(dir).pins);
        let image = pinned
            .get(&lok.host)
            .cloned()
            .unwrap_or_else(|| lok.image.clone());
        (lok.host.clone(), image)
    });
    let services = images(dir, config)
        .into_iter()
        .map(|(_, host, image)| (host, image));
    for (host, image) in lok.into_iter().chain(services) {
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
    // What each is prepared with is its image's to say. The coordination server is not one
    // of the hub's services, so it is asked here, for this.
    let mut said = described(dir, config).await;
    if let Some(lok) = config.running_lok() {
        if waiting.iter().any(|(service, _)| service == &lok.host) {
            let pinned = crate::pins::references(config, &crate::lock::read(dir).pins);
            let image = pinned.get(&lok.host).unwrap_or(&lok.image);
            if let Some(description) = describe(image).await {
                said.insert(lok.host.clone(), description);
            }
        }
    }
    let mut jobs = Vec::new();
    let mut done = Vec::new();
    let mut failed = Vec::new();
    for (service, id) in waiting {
        match said.get(&service) {
            None => failed.push(format!(
                "`{service}`'s image does not say how its database is prepared \
                 (run with no command, it has to say what it is), so nothing was started on it."
            )),
            // Nothing to prepare is prepared.
            Some(description) => match description.preparation() {
                None => {
                    prepared(dir, &service, &id).map_err(|error| error.to_string())?;
                    done.push(service);
                }
                Some(job) => jobs.push((service, id, job.to_vec())),
            },
        }
    }
    // Side by side: each service has a database of its own in the one server, and a
    // container of its own to prepare it from.
    let outcomes = futures_util::future::join_all(jobs.iter().map(|(service, _, job)| {
        say(format!(
            "Preparing {service}'s database for the build it runs"
        ));
        crate::compose::run_streamed(dir, prepare(service, job), on_line)
    }))
    .await;
    for ((service, id, _), outcome) in jobs.into_iter().zip(outcomes) {
        match outcome {
            Ok(_) => {
                prepared(dir, &service, &id).map_err(|error| error.to_string())?;
                done.push(service);
            }
            Err(said) => failed.push(format!(
                "`{service}`'s database could not be prepared for the build it would \
                 run, so nothing was started on it. It said:\n{said}"
            )),
        }
    }
    // The ones that went through are written down first: only the others are asked again.
    if !failed.is_empty() {
        return Err(failed.join("\n\n"));
    }
    Ok(done)
}

/// What the hub tells the service `id` about itself (`arkitekt_service.contract.facts.Facts`): the one
/// input its config is written from.
///
/// Nothing here is in the service's vocabulary. It is read off what this installer already
/// works out for every service — its database, its buckets, its key, how tokens are
/// verified — and off the other services: where each is, and what each offers
/// (`described`, by compose service; of one that did not describe itself, nothing is
/// said beyond where it is).
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
    let has = |block: &Option<Value>, name: &str| {
        block
            .as_ref()
            .is_some_and(|block| block.get(name).is_some())
    };
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
    ];
    // What it is registered as: its image's own word, remembered in its block.
    if let Some(identifier) = &service.identifier {
        me.push(("identifier", s(identifier)));
    }
    me.extend([
        ("secret_key", field(&django, "secret_key")),
        ("debug", field(&django, "debug")),
        ("allowed_hosts", field(&django, "hosts")),
    ]);
    if has(&django, "admin") {
        me.push(("admin", field(&django, "admin")));
    }
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

    // Its databases, by the name it asked for each under: none for a service that keeps
    // nothing in Postgres. Only what the service declared it uses is there to be told of.
    let mut databases = serde_norway::Mapping::new();
    if let Some(Value::Mapping(held)) = block("databases") {
        for (name, postgres) in held {
            let postgres = Some(postgres);
            databases.insert(
                name,
                map(vec![
                    ("host", field(&postgres, "host")),
                    ("port", field(&postgres, "port")),
                    ("name", field(&postgres, "db_name")),
                    ("username", field(&postgres, "username")),
                    ("password", field(&postgres, "password")),
                ]),
            );
        }
    }
    out.push(("databases", Value::Mapping(databases)));
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
    // Each secret it declared, as the file it is mounted at.
    if !service.secrets.is_empty() {
        out.push((
            "secrets",
            Value::Mapping(
                service
                    .secrets
                    .keys()
                    .map(|name| {
                        (
                            name.as_str().into(),
                            s(&crate::generate::service::secret_path(service, name)),
                        )
                    })
                    .collect(),
            ),
        ));
    }

    // --- the rest of the hub ----------------------------------------------------------
    // Whether this is the service that keeps the hub's catalogue: the one that is told
    // what every other one hosts.
    let catalogues = described
        .get(&service.host)
        .is_some_and(Description::catalogues);
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
        let mut entry = Vec::new();
        if let Some(identifier) = &peer.identifier {
            entry.push(("identifier", s(identifier)));
        }
        entry.extend([
            ("url", s(&base)),
            (
                "offers",
                Value::Mapping(offers.into_iter().map(|(k, v)| (k.into(), v)).collect()),
            ),
        ]);
        // What it hosts, as its image says it, for the service that catalogues it.
        let hosts = described
            .get(&peer.host)
            .map(|said| &said.hosts)
            .filter(|hosts| catalogues && !hosts.is_empty());
        if let Some(hosts) = hosts {
            entry.push((
                HOSTS,
                serde_norway::to_value(hosts).expect("what a service hosts is plain data"),
            ));
        }
        peers.push((peer.host.clone(), map(entry)));
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
        let mut config = build_hub_config(&HubConfigOptions::default());
        config.provide(&crate::support::said());
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
        assert_eq!(
            rekuest["databases"]["main"]["name"].as_str(),
            Some("rekuest_main")
        );
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
