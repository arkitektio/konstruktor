//! Apps attached beside a hub: any Arkitekt app, run as a container of its own, logged in as
//! itself, and given what it needs from this machine only when somebody agreed to it.
//!
//! An app says what it is in its **release**: a small document its release build pushes to
//! the app's registry repository under the release's tag (arkitekt-spec's
//! `ReleaseDescriptor`), read here from the registry without pulling a single image. It names
//! the app, the scopes and services it asks for, and its flavours — each an image pinned by
//! digest, where it may run (*selectors*), and what it must be granted where it runs
//! (*capabilities*: the container engine's socket, a device, a folder of this machine, …).
//!
//! A capability names the container side only. The host side is this machine's: the engine's
//! socket is found, a device or a folder is asked for. Those that give root-level access to
//! the machine — the container engine, a folder it may write to, the host's network, a Linux
//! capability — are granted only when asked for by name (`--allow`), never by a blanket yes.
//!
//! Attaching is the app device-code flow ([`crate::connect::app`]): somebody accepts the app
//! into an organization, and the client and refresh token that come back are what the
//! container logs in with. Attached to a hub, the app joins the hub's network, and the
//! organization that accepted it has to be one the hub is connected to.
//!
//! Every attached app is told the same few things about where it runs, whatever it is:
//!
//! | variable | what it is |
//! |---|---|
//! | `ARKITEKT_INSTANCE_ID` | which installation this is; also the container's hostname |
//! | `ARKITEKT_GATEWAY` | the coordination server, where the app and what it starts log in |
//! | `ARKITEKT_NETWORK` | the Docker network containers it starts are to join |
//! | `ARKITEKT_NETWORK_MODE` | on the mesh: the network mode they are to share instead |
//! | `ARKITEKT_DEVICE_ID` | this machine |
//!
//! The plugin engine is one such app: it is granted the container engine, and starts the apps
//! an organization installs through Kabinet on the network it is told.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_norway::Value;

use crate::config::mesh::{build_mesh_block, mesh_hostname, MeshBlock, MeshOptions};
use crate::connect::app::{self, AppEnvelope, AppManifest};
use crate::connect::manifest::ClientRequirement;
use crate::create::{now_rfc3339, CreateError, CreateEvent};
use crate::engine::{
    engine_secrets, hub_network_of, sidecar_container, sidecar_service, HUB_NETWORK_KEY, MESH_FILE,
    STATE_DIR,
};
use crate::engine_probe::EngineKind;
use crate::generate::service::{list, map, s};
use crate::generate::write::write_generated_files;
use crate::generate::GeneratedFiles;
use crate::{docker, registry};

/// The registry's `kind` for an attached app.
pub const KIND: &str = "app";

/// The app's record: where it was accepted, as what, and what it was granted. What marks a
/// folder as an app's.
pub const CONFIG_FILE: &str = "configs/app.yaml";

/// The credential the container logs in with (`FAKTS_TOKEN`).
pub const SECRETS_FILE: &str = "secrets/app.env";

/// The compose service the app runs under.
pub const SERVICE: &str = "app";

/// Where the app keeps its session and whatever else it keeps across restarts.
const STATE_VOLUME: &str = "app_state";

/// The media type of a manifest's config that marks it as an app's release.
pub const RELEASE_MEDIA_TYPE: &str = "application/vnd.arkitekt.release.v1+json";

/// Where the plugin engine is released.
pub const PLUGIN_ENGINE_RELEASE: &str = "ghcr.io/arkitektio/plugin_engine";

// --- what a release says -----------------------------------------------------------------

/// An app's release, as its registry repository carries it: as much of arkitekt-spec's
/// `ReleaseDescriptor` as attaching it reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub manifest: ReleaseManifest,
    #[serde(default)]
    pub flavours: Vec<Flavour>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub identifier: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// One flavour of a release: an image, where it may run, and what it must be granted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flavour {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Pinned by digest: what was inspected is what runs.
    pub image: String,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub selectors: Vec<serde_json::Value>,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub inspection: Inspection,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Inspection {
    /// The services the app asks for: the coordination server hands its client theirs.
    #[serde(default)]
    pub requirements: Vec<Requirement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    pub key: String,
    pub service: String,
    #[serde(default)]
    pub optional: bool,
}

/// What a flavour must be granted where it runs (arkitekt-spec's `Capability`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "yes")]
    pub required: bool,
    #[serde(flatten)]
    pub kind: CapabilityKind,
}

fn yes() -> bool {
    true
}

fn docker_socket() -> String {
    "/var/run/docker.sock".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CapabilityKind {
    /// The container engine's API socket, mounted at `target`.
    ContainerEngine {
        #[serde(default = "docker_socket")]
        target: String,
    },
    /// A device of this machine, appearing at `target`.
    Device {
        #[serde(rename = "class")]
        device_class: String,
        target: String,
    },
    /// A folder of this machine, mounted at `target`.
    Folder {
        target: String,
        #[serde(default = "yes", rename = "readOnly", alias = "read_only")]
        read_only: bool,
    },
    /// The host's network stack.
    HostNetwork,
    /// One Linux capability added to the container.
    LinuxCapability { cap: String },
    /// Shared memory of at least `min` MB.
    SharedMemory { min: u64 },
}

impl Capability {
    /// Whether granting it gives the app root-level access to this machine: granted only
    /// when asked for by name.
    pub fn gives_root(&self) -> bool {
        match &self.kind {
            CapabilityKind::ContainerEngine { .. }
            | CapabilityKind::HostNetwork
            | CapabilityKind::LinuxCapability { .. } => true,
            CapabilityKind::Folder { read_only, .. } => !read_only,
            CapabilityKind::Device { .. } | CapabilityKind::SharedMemory { .. } => false,
        }
    }

    /// What it hands the app, in the words a person agrees to.
    pub fn sentence(&self) -> String {
        match &self.kind {
            CapabilityKind::ContainerEngine { .. } => {
                "this machine's container engine — it can start, stop and inspect any container here: root on this machine".into()
            }
            CapabilityKind::Device { device_class, target } => {
                format!("a {device_class} device of this machine, as {target}")
            }
            CapabilityKind::Folder {
                target,
                read_only: true,
            } => format!("a folder of this machine to read, as {target}"),
            CapabilityKind::Folder {
                target,
                read_only: false,
            } => format!("a folder of this machine to write to, as {target} — anything in it"),
            CapabilityKind::HostNetwork => {
                "this machine's own network, instead of a network of its own".into()
            }
            CapabilityKind::LinuxCapability { cap } => {
                format!("the Linux capability {cap}")
            }
            CapabilityKind::SharedMemory { min } => format!("{min} MB of shared memory"),
        }
    }
}

/// The release `reference` (`registry/repository[:release]`) names, read from its registry.
/// Without a release, the newest one. Answers the reference as it was resolved.
pub async fn release(reference: &str) -> Result<(String, Release), CreateError> {
    let (repository, tag) = match split_reference(reference) {
        (repository, Some(tag)) => (repository, tag),
        (repository, None) => {
            let tags = tags(&repository).await.map_err(|error| {
                CreateError::Answers(format!("{repository} could not be read: {error}"))
            })?;
            let newest = newest_release(&tags)
                .ok_or_else(|| CreateError::Answers(format!("{repository} has no release yet")))?;
            (repository, newest)
        }
    };
    let resolved = format!("{repository}:{tag}");
    let unreadable = |error: String| CreateError::Answers(format!("{resolved}: {error}"));
    let mut registry = crate::oci::Registry::new(&resolved).map_err(unreadable)?;
    let manifest: crate::oci::Manifest = registry.manifest(&tag).await.map_err(unreadable)?;
    let config = manifest
        .config
        .filter(|config| config.media_type.as_deref() == Some(RELEASE_MEDIA_TYPE))
        .ok_or_else(|| {
            CreateError::Answers(format!(
                "{resolved} is not an Arkitekt app's release: name the app's repository and \
                 one of its releases, not one of its images"
            ))
        })?;
    let blob = registry.blob(&config.digest).await.map_err(unreadable)?;
    let release: Release = serde_json::from_slice(&blob)
        .map_err(|error| unreadable(format!("its release cannot be read ({error})")))?;
    Ok((resolved, release))
}

/// The tags of `repository`, from its registry.
async fn tags(repository: &str) -> Result<Vec<String>, String> {
    crate::oci::Registry::new(repository)?.tags().await
}

/// `registry/repository[:release]`, apart. A colon before the last slash is a port.
fn split_reference(reference: &str) -> (String, Option<String>) {
    let reference = reference.trim();
    match reference.rsplit_once(':') {
        Some((repository, tag)) if !tag.contains('/') && !tag.is_empty() => (
            repository.to_string(),
            Some(tag.trim_start_matches('v').to_string()),
        ),
        _ => (reference.to_string(), None),
    }
}

/// The newest of the release tags among `tags`: plain `X.Y.Z`, channels and builds aside.
fn newest_release(tags: &[String]) -> Option<String> {
    tags.iter()
        .filter_map(|tag| {
            let parts: Vec<u64> = tag
                .split('.')
                .map(|part| part.parse().ok())
                .collect::<Option<_>>()?;
            (parts.len() == 3).then(|| (parts, tag.clone()))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, tag)| tag)
}

// --- which flavour, on this machine ------------------------------------------------------

/// What this machine offers a flavour's selectors.
#[derive(Debug, Clone)]
pub struct Host {
    /// `amd64`, `arm64`.
    pub architecture: String,
    /// The engine runs NVIDIA containers.
    pub nvidia: bool,
    /// An AMD GPU is there for ROCm (`/dev/kfd`).
    pub amd: bool,
}

/// This machine, as far as selectors ask.
pub async fn host() -> Host {
    let runtimes = crate::engine_probe::engine()
        .async_command()
        .args(["info", "--format", "{{json .Runtimes}}"])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
        .unwrap_or_default();
    Host {
        architecture: crate::oci::host_platform().1.to_string(),
        nvidia: runtimes.contains("nvidia"),
        amd: Path::new("/dev/kfd").exists(),
    }
}

/// Why `flavour` cannot run on `host`, or nothing when it can.
pub fn misfit(flavour: &Flavour, host: &Host) -> Option<String> {
    let platform = format!("linux/{}", host.architecture);
    if !flavour.platforms.is_empty() && !flavour.platforms.iter().any(|p| p == &platform) {
        return Some(format!(
            "is built for {}, not for {platform}",
            flavour.platforms.join(", ")
        ));
    }
    for selector in &flavour.selectors {
        let required = selector
            .get("required")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        if !required {
            continue;
        }
        let kind = selector.get("kind").and_then(serde_json::Value::as_str);
        let unmet = match kind {
            Some("cpu") => selector
                .get("arch")
                .and_then(serde_json::Value::as_str)
                .filter(|arch| *arch != host.architecture)
                .map(|arch| format!("needs a {arch} CPU")),
            Some("cuda") => (!host.nvidia).then(|| "needs an NVIDIA GPU".to_string()),
            Some("rocm") => (!host.amd).then(|| "needs an AMD GPU".to_string()),
            Some("oneapi") => {
                Some("needs an Intel accelerator, which is not looked for here".into())
            }
            Some("label") => Some("needs a backend label this machine does not carry".into()),
            // Memory and core counts are the person's to judge here: nothing is refused for them.
            _ => None,
        };
        if unmet.is_some() {
            return unmet;
        }
    }
    None
}

/// The flavour to run: the one named, or the first that runs on `host`.
pub fn choose<'a>(
    release: &'a Release,
    named: Option<&str>,
    host: &Host,
) -> Result<&'a Flavour, CreateError> {
    if let Some(name) = named.map(str::trim).filter(|n| !n.is_empty()) {
        let flavour = release
            .flavours
            .iter()
            .find(|flavour| flavour.name == name)
            .ok_or_else(|| {
                CreateError::Answers(format!(
                    "{} has no flavour `{name}`; it has {}",
                    release.manifest.identifier,
                    names(release)
                ))
            })?;
        return match misfit(flavour, host) {
            Some(why) => Err(CreateError::Answers(format!(
                "The `{name}` flavour {why}, so it cannot run here."
            ))),
            None => Ok(flavour),
        };
    }
    let mut reasons = Vec::new();
    for flavour in &release.flavours {
        match misfit(flavour, host) {
            None => return Ok(flavour),
            Some(why) => reasons.push(format!("`{}` {why}", flavour.name)),
        }
    }
    Err(CreateError::Answers(if reasons.is_empty() {
        format!("{} has no flavour to run", release.manifest.identifier)
    } else {
        format!(
            "No flavour of {} runs here: {}.",
            release.manifest.identifier,
            reasons.join("; ")
        )
    }))
}

fn names(release: &Release) -> String {
    release
        .flavours
        .iter()
        .map(|f| format!("`{}`", f.name))
        .collect::<Vec<_>>()
        .join(", ")
}

// --- what is granted ---------------------------------------------------------------------

/// Everything a front end collects to attach an app.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AttachAnswers {
    pub dir: String,
    pub name: String,
    /// The app's release: `registry/repository[:release]`.
    pub release: String,
    /// The flavour to run. Left out, the first that runs here.
    #[serde(default)]
    pub flavour: Option<String>,
    /// The coordination server to be accepted at. Left out, the one `hub` is connected to.
    #[serde(default)]
    pub coord_server: Option<String>,
    /// A hub on this machine: the app joins its network, and has to be accepted into one of
    /// the organizations the hub is connected to.
    #[serde(default)]
    pub hub: Option<String>,
    /// Put the app on the mesh, with a key asked for with the grant.
    #[serde(default)]
    pub mesh: bool,
    /// Which installation this is, e.g. to take over from a plugin engine a hub ran itself,
    /// whose containers are marked by it. Left out, the deployment's name.
    #[serde(default)]
    pub instance_id: Option<String>,
    /// Capabilities that give root-level access, granted by name.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Optional capabilities left out.
    #[serde(default)]
    pub decline: Vec<String>,
    /// The device of this machine for each `device` capability, by name.
    #[serde(default)]
    pub devices: BTreeMap<String, String>,
    /// The folder of this machine for each `folder` capability, by name.
    #[serde(default)]
    pub folders: BTreeMap<String, String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Start it once it is written.
    #[serde(default)]
    pub start: bool,
}

/// A capability as granted: with the host side, for a device or a folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Granted {
    pub capability: Capability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

/// What of `flavour`'s capabilities is granted by `answers` — or why it cannot be attached
/// as they stand. An optional capability not granted is left out; a required one refuses.
pub fn grant(flavour: &Flavour, answers: &AttachAnswers) -> Result<Vec<Granted>, CreateError> {
    let mut granted = Vec::new();
    let mut missing = Vec::new();
    for capability in &flavour.capabilities {
        let name = capability.name.as_str();
        if answers.decline.iter().any(|d| d == name) {
            if capability.required {
                missing.push(format!(
                    "`{name}` cannot be declined: the app does not run without {}",
                    capability.sentence()
                ));
            }
            continue;
        }
        let host = match &capability.kind {
            CapabilityKind::Device { .. } => match answers.devices.get(name) {
                Some(device) => Some(device.clone()),
                None => {
                    if capability.required {
                        missing.push(format!(
                            "`{name}` wants {} — name which: --device {name}=/dev/…",
                            capability.sentence()
                        ));
                    }
                    continue;
                }
            },
            CapabilityKind::Folder { .. } => match answers.folders.get(name) {
                Some(folder) => Some(folder.clone()),
                None => {
                    if capability.required {
                        missing.push(format!(
                            "`{name}` wants {} — name which: --folder {name}=/a/folder",
                            capability.sentence()
                        ));
                    }
                    continue;
                }
            },
            _ => None,
        };
        if capability.gives_root() && !answers.allow.iter().any(|a| a == name) {
            if capability.required {
                missing.push(format!(
                    "`{name}` wants {} — grant it by name: --allow {name}",
                    capability.sentence()
                ));
            }
            continue;
        }
        granted.push(Granted {
            capability: capability.clone(),
            host,
        });
    }
    if !missing.is_empty() {
        return Err(CreateError::Answers(format!(
            "Nothing was written: {}.",
            missing.join("; ")
        )));
    }
    let on_host_network = granted
        .iter()
        .any(|g| g.capability.kind == CapabilityKind::HostNetwork);
    if on_host_network && (answers.mesh || answers.hub.is_some()) {
        return Err(CreateError::Answers(
            "An app on this machine's own network joins no other: attach it without --hub \
             and --mesh, or without granting the host network."
                .into(),
        ));
    }
    Ok(granted)
}

// --- what is written ---------------------------------------------------------------------

/// What an app's compose file is built from.
#[derive(Debug, Clone)]
pub struct AppCompose<'a> {
    pub flavour: &'a Flavour,
    pub granted: &'a [Granted],
    pub instance_id: &'a str,
    pub device_id: Option<&'a str>,
    pub coord_server: &'a str,
    /// The app's own compose project, whose network what it starts joins when it is not
    /// attached to a hub.
    pub project: &'a str,
    pub hub_network: Option<&'a str>,
    pub mesh: Option<&'a MeshBlock>,
    pub engine: EngineKind,
    /// This machine's side of the engine's socket, when it is not the usual one.
    pub host_socket: Option<&'a str>,
}

/// The `docker-compose.yaml` an app deployment consists of: the app, with exactly what it
/// was granted, and on the mesh the sidecar it shares a network with.
pub fn build_app_compose(compose: &AppCompose<'_>) -> Value {
    let fakts_url = crate::connect::wellknown::base_url(compose.coord_server);
    let network = match compose.hub_network {
        Some(network) => network.to_string(),
        None => format!("{}_default", compose.project),
    };
    let mut environment = vec![
        ("FAKTS_URL", s(&fakts_url)),
        ("ARKITEKT_GATEWAY", s(&fakts_url)),
        ("ARKITEKT_NETWORK", s(&network)),
        ("ARKITEKT_INSTANCE_ID", s(compose.instance_id)),
        ("ARKITEKT_HEADLESS", s("1")),
        ("XDG_STATE_HOME", s(STATE_DIR)),
    ];
    if let Some(device_id) = compose.device_id {
        environment.push(("ARKITEKT_DEVICE_ID", s(device_id)));
    }
    if let Some(mesh) = compose.mesh {
        environment.push((
            "ARKITEKT_NETWORK_MODE",
            s(&format!(
                "container:{}",
                sidecar_container(compose.project, mesh)
            )),
        ));
    }

    let mut volumes = vec![s(&format!("{STATE_VOLUME}:{STATE_DIR}"))];
    let mut devices = Vec::new();
    let mut cap_add = Vec::new();
    let mut shm_size = None;
    let mut host_network = false;
    for granted in compose.granted {
        let host = granted.host.as_deref().unwrap_or_default();
        match &granted.capability.kind {
            CapabilityKind::ContainerEngine { target } => {
                let socket = compose
                    .host_socket
                    .unwrap_or_else(|| compose.engine.container_socket());
                volumes.push(s(&format!("{socket}:{target}")));
                environment.push(("DOCKER_HOST", s(&format!("unix://{target}"))));
            }
            CapabilityKind::Device { target, .. } => devices.push(s(&format!("{host}:{target}"))),
            CapabilityKind::Folder { target, read_only } => volumes.push(s(&if *read_only {
                format!("{host}:{target}:ro")
            } else {
                format!("{host}:{target}")
            })),
            CapabilityKind::HostNetwork => host_network = true,
            CapabilityKind::LinuxCapability { cap } => cap_add.push(s(cap)),
            CapabilityKind::SharedMemory { min } => shm_size = Some(*min),
        }
    }
    // A GPU is a selector, not a capability: a flavour placed on one is given it.
    let mut gpus = None;
    for selector in &compose.flavour.selectors {
        match selector.get("kind").and_then(serde_json::Value::as_str) {
            Some("cuda") => {
                let count = selector
                    .get("count")
                    .and_then(serde_json::Value::as_u64)
                    .map_or_else(|| s("all"), |n| Value::Number(n.into()));
                gpus = Some(map(vec![(
                    "resources",
                    map(vec![(
                        "reservations",
                        map(vec![(
                            "devices",
                            list(vec![map(vec![
                                ("driver", s("nvidia")),
                                ("count", count),
                                ("capabilities", list(vec![s("gpu")])),
                            ])]),
                        )]),
                    )]),
                )]));
            }
            Some("rocm") => {
                devices.push(s("/dev/kfd:/dev/kfd"));
                devices.push(s("/dev/dri:/dev/dri"));
            }
            _ => {}
        }
    }

    let mut app = vec![
        ("image", s(&compose.flavour.image)),
        ("command", list(vec![s("arkitekt"), s("run"), s("prod")])),
        ("restart", s("unless-stopped")),
        ("stop_grace_period", s("2s")),
        ("environment", map(environment)),
        ("env_file", list(vec![s(&format!("./{SECRETS_FILE}"))])),
        ("volumes", list(volumes)),
    ];
    if !devices.is_empty() {
        app.push(("devices", list(devices)));
    }
    if !cap_add.is_empty() {
        app.push(("cap_add", list(cap_add)));
    }
    if let Some(min) = shm_size {
        app.push(("shm_size", s(&format!("{min}m"))));
    }
    if let Some(deploy) = gpus {
        app.push(("deploy", deploy));
    }

    let joins = compose
        .hub_network
        .map(|_| ("networks", list(vec![s("default"), s(HUB_NETWORK_KEY)])));
    let mut services = vec![];
    let mut document = vec![];
    let mut named_volumes = vec![(STATE_VOLUME, map(vec![]))];
    if host_network {
        app.push(("network_mode", s("host")));
    } else {
        match compose.mesh {
            Some(mesh) => {
                app.push(("network_mode", s(&format!("service:{}", mesh.host))));
                app.push(("depends_on", list(vec![s(&mesh.host)])));
                services.push((mesh.host.as_str(), sidecar_service(mesh, joins)));
                named_volumes.push((mesh.volume_name.as_str(), map(vec![])));
            }
            None => {
                // Its own name, which what it marks as its own goes by too — the same after
                // it is recreated.
                app.push(("hostname", s(compose.instance_id)));
                app.extend(joins);
            }
        }
    }
    document.push(("volumes", map(named_volumes)));
    services.insert(0, (SERVICE, map(app)));
    if let Some(network) = compose.hub_network {
        // The hub's, declared external: a `down` here must never take it from the hub.
        document.push((
            "networks",
            map(vec![(
                HUB_NETWORK_KEY,
                map(vec![("external", Value::Bool(true)), ("name", s(network))]),
            )]),
        ));
    }
    document.insert(0, ("services", map(services)));
    map(document)
}

/// The app's record ([`CONFIG_FILE`]): its login, what it is, and what it was granted. What
/// an update reads back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRecord {
    pub fakts: RecordedLogin,
    pub app: RecordedApp,
    #[serde(default)]
    pub granted: Vec<Granted>,
}

/// The client the app was accepted as, and the refresh token it was handed then — spent on
/// its first login, from which it lives on the chain it keeps in its state volume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedLogin {
    pub endpoint_url: String,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedApp {
    pub identifier: String,
    pub version: String,
    pub instance_id: String,
    /// The release it runs, as resolved: `registry/repository:X.Y.Z`.
    pub release: String,
    pub flavour: String,
    pub image: String,
    /// The organization that accepted it. Left out by records from before updates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
}

/// The record in `dir`.
pub fn read_app_record(dir: &Path) -> Result<AppRecord, CreateError> {
    let path = dir.join(CONFIG_FILE);
    let text = std::fs::read_to_string(&path).map_err(|_| {
        CreateError::Answers(format!(
            "{} holds no attached app ({CONFIG_FILE} is missing). A plugin engine created \
             before apps could be attached is attached anew: `konstruktor engine create`.",
            dir.display()
        ))
    })?;
    serde_norway::from_str(&text).map_err(|error| {
        CreateError::Answers(format!("{} cannot be read: {error}", path.display()))
    })
}

/// What the app says of itself when it asks to be let in: what its release says.
pub fn app_manifest(
    release: &Release,
    flavour: &Flavour,
    instance_id: &str,
    description: Option<String>,
    node_id: Option<String>,
) -> AppManifest {
    AppManifest {
        identifier: release.manifest.identifier.clone(),
        version: release.manifest.version.clone(),
        description: description.or_else(|| release.manifest.description.clone()),
        logo: release.manifest.logo.clone(),
        scopes: release.manifest.scopes.clone(),
        requirements: flavour
            .inspection
            .requirements
            .iter()
            .map(|requirement| ClientRequirement {
                key: requirement.key.clone(),
                service: requirement.service.clone(),
            })
            .collect(),
        instance_id: instance_id.to_string(),
        node_id,
    }
}

// --- attaching ---------------------------------------------------------------------------

/// What attaching wrote.
pub struct AttachedApp {
    pub path: PathBuf,
    pub record: registry::DeploymentRecord,
    pub release: String,
    pub flavour: String,
    pub granted: Vec<Granted>,
    /// The organization that accepted the app.
    pub organization: Option<String>,
}

/// Read the release → choose the flavour → grant → accept → write → register → start.
///
/// Everything that can refuse does so before anybody is sent to a browser: a release that
/// cannot be read, no flavour that runs here, a capability not granted. Once the app is
/// accepted, what comes back is what the container logs in with.
pub async fn attach_app(
    answers: &AttachAnswers,
    cancel: &tokio_util::sync::CancellationToken,
    on: &(dyn Fn(CreateEvent) + Sync),
) -> Result<AttachedApp, CreateError> {
    docker_ready(on).await?;

    let dir = PathBuf::from(&answers.dir);
    std::fs::create_dir_all(&dir)?;
    let mut store = registry::load();
    let verdict = registry::inspect_folder(&store, &dir);
    if !verdict.can_create() {
        return Err(CreateError::Folder(verdict.describe()));
    }

    let hub = answers
        .hub
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(PathBuf::from);
    let hub_network = hub.as_deref().map(hub_network_of).transpose()?;
    let hub_grants = hub
        .as_deref()
        .and_then(crate::credentials::read_credentials);
    let coord_server = match (
        answers.coord_server.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        &hub_grants,
    ) {
        (Some(given), Some(held)) if held.server.trim() != given => {
            return Err(CreateError::Answers(format!(
                "The hub is connected to {}, so an app beside it is accepted there, not at {given}.",
                held.server
            )))
        }
        (Some(given), _) => given.to_string(),
        (None, Some(held)) => held.server.clone(),
        (None, None) => {
            return Err(CreateError::Answers(
                "Name the coordination server to accept the app at, or attach it to a hub \
                 that is connected to one."
                    .into(),
            ))
        }
    };

    on(CreateEvent::Building);
    on(CreateEvent::Log {
        line: format!("Reading the release of {}…", answers.release.trim()),
    });
    let (reference, release) = release(&answers.release).await?;
    let flavour = choose(&release, answers.flavour.as_deref(), &host().await)?;
    let grants = grant(flavour, answers)?;

    let instance_id = answers
        .instance_id
        .as_deref()
        .map(str::trim)
        .filter(|i| !i.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| crate::create::identifier_from_folder(&dir));
    let manifest = app_manifest(
        &release,
        flavour,
        &instance_id,
        answers
            .description
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(str::to_string),
        Some(store.device_id.clone()),
    );

    let accepted = accept(
        &coord_server,
        &manifest,
        answers.mesh,
        hub_grants.as_ref(),
        cancel,
        on,
    )
    .await?;
    let files = app_files(&Placement {
        dir: &dir,
        coord_server: &coord_server,
        reference: &reference,
        release: &release,
        flavour,
        instance_id: &instance_id,
        grants: &grants,
        accepted: &accepted,
        hub_network: hub_network.as_deref(),
        device_id: &store.device_id,
    })
    .await;
    for name in files.keys() {
        on(CreateEvent::Writing { file: name.clone() });
    }
    write_generated_files(&dir, &files)?;

    let record = registry::register_kind(
        &mut store,
        KIND,
        &answers.name,
        &dir.to_string_lossy(),
        Some(coord_server.clone()),
        Some(instance_id.clone()),
        now_rfc3339(),
    );
    let _ = registry::save(&store);

    if answers.start {
        on(CreateEvent::Starting);
        let log = |line: crate::compose::ComposeLine| on(CreateEvent::Log { line: line.line });
        if let Err(error) = crate::start::start(&dir, &log).await {
            on(CreateEvent::Log {
                line: error.to_string(),
            });
            return Err(CreateError::StartFailed);
        }
    }

    on(CreateEvent::Done {
        path: dir.to_string_lossy().to_string(),
    });
    Ok(AttachedApp {
        path: dir,
        record,
        release: reference,
        flavour: flavour.name.clone(),
        granted: grants,
        organization: accepted.organization,
    })
}

async fn docker_ready(on: &(dyn Fn(CreateEvent) + Sync)) -> Result<(), CreateError> {
    on(CreateEvent::CheckingDocker);
    let probe = docker::probe().await;
    if probe.is_ready() {
        Ok(())
    } else {
        Err(CreateError::Docker(crate::create::describe_docker(&probe)))
    }
}

/// What accepting the app handed back.
struct Accepted {
    granted: AppEnvelope,
    organization: Option<String>,
    /// On the mesh: the sidecar it shares a network with, keyed by what came with the grant.
    mesh: Option<MeshBlock>,
}

/// The app device-code flow: somebody accepts `manifest` into an organization — one the hub
/// is connected to, when the app runs beside one.
async fn accept(
    coord_server: &str,
    manifest: &AppManifest,
    mesh: bool,
    hub_grants: Option<&crate::credentials::HubCredentials>,
    cancel: &tokio_util::sync::CancellationToken,
    on: &(dyn Fn(CreateEvent) + Sync),
) -> Result<Accepted, CreateError> {
    let grant = app::start(coord_server, manifest, mesh).await?;
    on(CreateEvent::Staged {
        user_code: grant.user_code.clone(),
        verification_uri_complete: grant.verification_uri_complete.clone(),
        expires_in: grant.expires_in,
    });
    let granted = app::wait_for_app(&grant, cancel, &|progress| {
        on(CreateEvent::Waiting {
            polls: progress.polls,
            seconds_left: progress.seconds_left,
        })
    })
    .await?;
    if granted
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .is_none()
    {
        return Err(CreateError::AppAuthorization(
            app::AppAuthorizationError::NoRefreshToken {
                fields: granted.declared_fields(),
            },
        ));
    }
    let organization = granted
        .self_
        .as_ref()
        .and_then(|me| me.organization.clone());
    if let (Some(held), Some(organization)) = (hub_grants, &organization) {
        let served = held.organizations();
        if !served.is_empty() && !served.contains(organization) {
            return Err(CreateError::Answers(format!(
                "The app was accepted into organization {organization}, which the hub is not \
                 connected to (it serves {}): nothing was written. Remove the app there, and \
                 accept it into one of the hub's organizations.",
                served.join(", ")
            )));
        }
    }
    on(CreateEvent::Granted {
        mesh_key: granted.mesh_key().is_some(),
    });
    let mesh = if mesh {
        let (key, coord_url) = granted.mesh_key().ok_or(CreateError::EngineNoMeshKey)?;
        Some(build_mesh_block(&MeshOptions {
            hostname: mesh_hostname(&manifest.instance_id),
            auth_key: key.to_string(),
            coord_url: coord_url.map(str::to_string),
            login: granted.login(),
        }))
    } else {
        None
    };
    Ok(Accepted {
        granted,
        organization,
        mesh,
    })
}

/// Where and as what an accepted app is placed.
struct Placement<'a> {
    dir: &'a Path,
    coord_server: &'a str,
    reference: &'a str,
    release: &'a Release,
    flavour: &'a Flavour,
    instance_id: &'a str,
    grants: &'a [Granted],
    accepted: &'a Accepted,
    hub_network: Option<&'a str>,
    device_id: &'a str,
}

/// The files an accepted app's folder consists of — the same whether it was just attached
/// or updated in place.
async fn app_files(placement: &Placement<'_>) -> GeneratedFiles {
    let engine = crate::engine_probe::engine();
    let host_socket = docker::host_socket(&engine).await;
    let project = crate::compose::project_name(&placement.dir.to_string_lossy());
    let granted = &placement.accepted.granted;
    let mut files = GeneratedFiles::new();
    files.insert(
        "docker-compose.yaml".to_string(),
        crate::generate::dump(&build_app_compose(&AppCompose {
            flavour: placement.flavour,
            granted: placement.grants,
            instance_id: placement.instance_id,
            device_id: Some(placement.device_id),
            coord_server: placement.coord_server,
            project: &project,
            hub_network: placement.hub_network,
            mesh: placement.accepted.mesh.as_ref(),
            engine: engine.kind,
            host_socket: host_socket.as_deref(),
        })),
    );
    let record = AppRecord {
        fakts: RecordedLogin {
            endpoint_url: crate::connect::wellknown::base_url(placement.coord_server),
            client_id: granted.client_id.clone(),
            client_secret: granted.client_secret.clone().filter(|v| !v.is_empty()),
            refresh_token: granted.refresh_token.clone().filter(|v| !v.is_empty()),
        },
        app: RecordedApp {
            identifier: placement.release.manifest.identifier.clone(),
            version: placement.release.manifest.version.clone(),
            instance_id: placement.instance_id.to_string(),
            release: placement.reference.to_string(),
            flavour: placement.flavour.name.clone(),
            image: placement.flavour.image.clone(),
            organization: placement.accepted.organization.clone(),
        },
        granted: placement.grants.to_vec(),
    };
    files.insert(
        CONFIG_FILE.to_string(),
        serde_norway::to_string(&record).expect("an app record serializes"),
    );
    if let Some(secrets) = engine_secrets(&granted.client_id, granted.refresh_token.as_deref()) {
        files.insert(SECRETS_FILE.to_string(), secrets);
    }
    if let Some(mesh) = &placement.accepted.mesh {
        files.insert(
            MESH_FILE.to_string(),
            serde_norway::to_string(mesh).expect("a mesh block serializes"),
        );
    }
    files
}

// --- updating ----------------------------------------------------------------------------

/// What a front end collects to update an attached app.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateAnswers {
    pub dir: String,
    /// The release to move to, `X.Y.Z`. Left out, the newest.
    #[serde(default)]
    pub to: Option<String>,
    /// Capabilities the new release asks for anew that give root-level access, by name.
    /// Those it was granted before, unchanged, are kept without asking.
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub decline: Vec<String>,
    #[serde(default)]
    pub devices: BTreeMap<String, String>,
    #[serde(default)]
    pub folders: BTreeMap<String, String>,
}

/// The release an app would move to, and what changes in what it is granted.
#[derive(Debug, Clone)]
pub struct UpdatePlan {
    pub record: AppRecord,
    pub reference: String,
    pub release: Release,
    /// The flavour it moves to: its own, when the release still has it and it runs here.
    pub flavour: String,
    /// Granted before and asked for unchanged: kept.
    pub kept: Vec<String>,
    /// Asked for anew, or changed: granted only as the answers say.
    pub asked: Vec<Capability>,
    /// Granted before, no longer asked for: dropped.
    pub dropped: Vec<String>,
}

impl UpdatePlan {
    /// Whether the release is one the app does not run yet.
    pub fn is_newer(&self) -> bool {
        newer(&self.release.manifest.version, &self.record.app.version)
    }

    fn flavour(&self) -> &Flavour {
        self.release
            .flavours
            .iter()
            .find(|f| f.name == self.flavour)
            .expect("the plan's flavour is the release's")
    }
}

/// Whether version `a` comes after `b`, both `X.Y.Z`. Anything else is never newer.
pub fn newer(a: &str, b: &str) -> bool {
    match (release_number(a), release_number(b)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn release_number(version: &str) -> Option<Vec<u64>> {
    let parts: Vec<u64> = version
        .trim_start_matches('v')
        .split('.')
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    (parts.len() == 3).then_some(parts)
}

/// What updating the app in `dir` would do: the release, and what it asks for that it was
/// not granted. Reads the registry, writes nothing.
pub async fn plan_update(dir: &Path, to: Option<&str>) -> Result<UpdatePlan, CreateError> {
    let record = read_app_record(dir)?;
    let (repository, _) = split_reference(&record.app.release);
    let wanted = match to.map(str::trim).filter(|v| !v.is_empty()) {
        Some(version) => format!("{repository}:{}", version.trim_start_matches('v')),
        None => repository,
    };
    let (reference, release) = release(&wanted).await?;
    if release.manifest.identifier != record.app.identifier {
        return Err(CreateError::Answers(format!(
            "{reference} is {}, not {}: an update stays the same app.",
            release.manifest.identifier, record.app.identifier
        )));
    }
    let host = host().await;
    let flavour = match choose(&release, Some(&record.app.flavour), &host) {
        Ok(flavour) => flavour,
        Err(_) => choose(&release, None, &host)?,
    };

    let mut kept = Vec::new();
    let mut asked = Vec::new();
    for capability in &flavour.capabilities {
        if record.granted.iter().any(|g| &g.capability == capability) {
            kept.push(capability.name.clone());
        } else {
            asked.push(capability.clone());
        }
    }
    let dropped = record
        .granted
        .iter()
        .map(|g| g.capability.name.clone())
        .filter(|name| !kept.contains(name))
        .collect();
    let flavour = flavour.name.clone();
    Ok(UpdatePlan {
        record,
        reference,
        release,
        flavour,
        kept,
        asked,
        dropped,
    })
}

/// The grants for `plan`'s flavour: what was granted before and is asked for unchanged, as
/// it was granted — its host side too — and the rest as `answers` say.
pub fn regrant(plan: &UpdatePlan, answers: &UpdateAnswers) -> Result<Vec<Granted>, CreateError> {
    let mut attach = AttachAnswers {
        allow: answers.allow.clone(),
        decline: answers.decline.clone(),
        devices: answers.devices.clone(),
        folders: answers.folders.clone(),
        ..Default::default()
    };
    for previous in &plan.record.granted {
        if !plan.kept.contains(&previous.capability.name) {
            continue;
        }
        let name = previous.capability.name.clone();
        match (&previous.capability.kind, &previous.host) {
            (CapabilityKind::Device { .. }, Some(host)) => {
                attach.devices.entry(name.clone()).or_insert(host.clone());
            }
            (CapabilityKind::Folder { .. }, Some(host)) => {
                attach.folders.entry(name.clone()).or_insert(host.clone());
            }
            _ => {}
        }
        if previous.capability.gives_root() {
            attach.allow.push(name);
        }
    }
    let mesh = Path::new(&answers.dir).join(MESH_FILE).is_file();
    attach.mesh = mesh;
    attach.hub = crate::engine::attached_network(Path::new(&answers.dir));
    grant(plan.flavour(), &attach)
}

/// What updating did.
pub struct UpdatedApp {
    pub path: PathBuf,
    pub from: String,
    pub to: String,
    pub release: String,
    pub flavour: String,
    pub granted: Vec<Granted>,
    pub organization: Option<String>,
}

/// Move the app in `dir` to `plan`'s release, in place: accepted again — the new release is
/// a new client at the coordination server, and the session the app keeps is its old
/// version's — then the files rewritten and the container recreated. The instance id, the
/// state volume and the mesh node stay; so what the app started stays its own.
///
/// Nothing changes until the new release is accepted. If it does not come up, the previous
/// files are put back and the previous release started again.
pub async fn update_app(
    plan: &UpdatePlan,
    answers: &UpdateAnswers,
    cancel: &tokio_util::sync::CancellationToken,
    on: &(dyn Fn(CreateEvent) + Sync),
) -> Result<UpdatedApp, CreateError> {
    docker_ready(on).await?;
    let dir = PathBuf::from(&answers.dir);
    let grants = regrant(plan, answers)?;
    let flavour = plan.flavour();
    let mut store = registry::load();
    let coord_server = registry::find_by_path(&store, &dir.to_string_lossy())
        .and_then(|record| record.coord_server.clone())
        .unwrap_or_else(|| plan.record.fakts.endpoint_url.clone());
    let hub = crate::engine::attached_hub(&dir).map(|record| PathBuf::from(record.path));
    let hub_grants = hub
        .as_deref()
        .and_then(crate::credentials::read_credentials);
    let hub_network = crate::engine::attached_network(&dir);
    let instance_id = plan.record.app.instance_id.clone();
    let manifest = app_manifest(
        &plan.release,
        flavour,
        &instance_id,
        None,
        Some(store.device_id.clone()),
    );

    on(CreateEvent::Log {
        line: format!(
            "{} {} → {}: accept it again.",
            plan.record.app.identifier, plan.record.app.version, plan.release.manifest.version
        ),
    });
    let mesh = dir.join(MESH_FILE).is_file();
    let accepted = accept(
        &coord_server,
        &manifest,
        mesh,
        hub_grants.as_ref(),
        cancel,
        on,
    )
    .await?;
    if let (Some(before), Some(now)) = (&plan.record.app.organization, &accepted.organization) {
        if before != now {
            return Err(CreateError::Answers(format!(
                "The update was accepted into organization {now}, but the app runs for \
                 {before}: nothing was changed. Remove it there, and accept it into {before}."
            )));
        }
    }

    let files = app_files(&Placement {
        dir: &dir,
        coord_server: &coord_server,
        reference: &plan.reference,
        release: &plan.release,
        flavour,
        instance_id: &instance_id,
        grants: &grants,
        accepted: &accepted,
        hub_network: hub_network.as_deref(),
        device_id: &store.device_id,
    })
    .await;
    // Started again only if it ran: an app somebody stopped stays stopped.
    let running = crate::compose::is_up(&dir).await;
    // What is put back if the new release does not come up.
    let previous: GeneratedFiles = files
        .keys()
        .filter_map(|name| {
            std::fs::read_to_string(dir.join(name))
                .ok()
                .map(|text| (name.clone(), text))
        })
        .collect();
    for name in files.keys() {
        on(CreateEvent::Writing { file: name.clone() });
    }
    write_generated_files(&dir, &files)?;

    let log = |line: crate::compose::ComposeLine| on(CreateEvent::Log { line: line.line });
    if running {
        on(CreateEvent::Starting);
    }
    if let Err(error) = match running {
        true => crate::start::start(&dir, &log).await.map(|_| ()),
        false => Ok(()),
    } {
        on(CreateEvent::Log {
            line: format!(
                "{error} — putting {} {} back.",
                plan.record.app.identifier, plan.record.app.version
            ),
        });
        write_generated_files(&dir, &previous)?;
        for name in files.keys().filter(|name| !previous.contains_key(*name)) {
            let _ = std::fs::remove_file(dir.join(name));
        }
        let _ = crate::start::start(&dir, &log).await;
        return Err(CreateError::StartFailed);
    }

    registry::record_regeneration(
        &mut store,
        &dir.to_string_lossy(),
        Some(coord_server.clone()),
        Some(instance_id),
        now_rfc3339(),
    );
    let _ = registry::save(&store);
    on(CreateEvent::Done {
        path: dir.to_string_lossy().to_string(),
    });
    Ok(UpdatedApp {
        path: dir,
        from: plan.record.app.version.clone(),
        to: plan.release.manifest.version.clone(),
        release: plan.reference.clone(),
        flavour: flavour.name.clone(),
        granted: grants,
        organization: accepted.organization,
    })
}

/// The newest release of the app in `dir` when it is newer than the one it runs: what
/// `status` shows as an update. `None` when it is up to date, or that cannot be told.
pub async fn available_update(dir: &Path) -> Option<String> {
    let record = read_app_record(dir).ok()?;
    let (repository, _) = split_reference(&record.app.release);
    let tags = tags(&repository).await.ok()?;
    newest_release(&tags).filter(|newest| newer(newest, &record.app.version))
}

/// Whether `dir` holds an attached app.
pub fn holds_an_app(dir: &Path) -> bool {
    dir.join(CONFIG_FILE).is_file()
}

/// One value of an app's record, by its section and key.
pub fn app_record(dir: &Path, section: &str, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(CONFIG_FILE)).ok()?;
    let config: Value = serde_norway::from_str(&text).ok()?;
    config.get(section)?.get(key)?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flavour(capabilities: serde_json::Value, selectors: serde_json::Value) -> Flavour {
        serde_json::from_value(serde_json::json!({
            "name": "vanilla",
            "image": "ghcr.io/arkitektio/plugin_engine@sha256:aaaa",
            "platforms": ["linux/amd64", "linux/arm64"],
            "selectors": selectors,
            "capabilities": capabilities,
            "inspection": {"requirements": [{"key": "kabinet", "service": "live.arkitekt.kabinet"}]},
        }))
        .expect("a flavour")
    }

    fn engine_flavour() -> Flavour {
        flavour(
            serde_json::json!([{"kind": "container_engine", "name": "engine"}]),
            serde_json::json!([]),
        )
    }

    fn here() -> Host {
        Host {
            architecture: "amd64".into(),
            nvidia: false,
            amd: false,
        }
    }

    #[test]
    fn a_reference_names_a_release_or_leaves_the_newest_to_be_found() {
        assert_eq!(
            split_reference("ghcr.io/arkitektio/plugin_engine:2.1.0"),
            (
                "ghcr.io/arkitektio/plugin_engine".into(),
                Some("2.1.0".into())
            )
        );
        assert_eq!(
            split_reference("ghcr.io/arkitektio/plugin_engine:v2.1.0")
                .1
                .as_deref(),
            Some("2.1.0")
        );
        assert_eq!(split_reference("localhost:5000/app").1, None);
        let tags = ["main", "2.0.0", "2.10.0", "2.9.1", "2.1.1-dev.3f2a-vanilla"].map(String::from);
        assert_eq!(newest_release(&tags).as_deref(), Some("2.10.0"));
    }

    #[test]
    fn the_container_engine_is_granted_by_name_only() {
        let flavour = engine_flavour();
        let refused = grant(&flavour, &AttachAnswers::default())
            .unwrap_err()
            .to_string();
        assert!(refused.contains("--allow engine"), "{refused}");

        let answers = AttachAnswers {
            allow: vec!["engine".into()],
            ..Default::default()
        };
        assert_eq!(grant(&flavour, &answers).unwrap().len(), 1);
    }

    #[test]
    fn a_folder_without_its_host_side_is_asked_for_and_an_optional_one_left_out() {
        let flavour = flavour(
            serde_json::json!([
                {"kind": "folder", "name": "data", "target": "/data"},
                {"kind": "device", "name": "camera", "class": "video", "target": "/dev/video0", "required": false},
            ]),
            serde_json::json!([]),
        );
        let refused = grant(&flavour, &AttachAnswers::default())
            .unwrap_err()
            .to_string();
        assert!(refused.contains("--folder data="), "{refused}");
        assert!(
            !refused.contains("camera"),
            "an optional one is not demanded: {refused}"
        );

        let answers = AttachAnswers {
            folders: BTreeMap::from([("data".to_string(), "/srv/data".to_string())]),
            ..Default::default()
        };
        let granted = grant(&flavour, &answers).unwrap();
        assert_eq!(granted.len(), 1);
        assert_eq!(granted[0].host.as_deref(), Some("/srv/data"));
    }

    #[test]
    fn a_writable_folder_gives_root_and_a_readable_one_does_not() {
        let flavour = flavour(
            serde_json::json!([{"kind": "folder", "name": "data", "target": "/data", "readOnly": false}]),
            serde_json::json!([]),
        );
        let answers = AttachAnswers {
            folders: BTreeMap::from([("data".to_string(), "/srv/data".to_string())]),
            ..Default::default()
        };
        assert!(grant(&flavour, &answers).is_err());
        assert!(flavour.capabilities[0].gives_root());
    }

    #[test]
    fn the_compose_file_holds_exactly_what_was_granted() {
        let flavour = flavour(
            serde_json::json!([
                {"kind": "container_engine", "name": "engine"},
                {"kind": "device", "name": "camera", "class": "video", "target": "/dev/video0"},
                {"kind": "shared_memory", "name": "shm", "min": 512},
            ]),
            serde_json::json!([{"kind": "cuda"}]),
        );
        let granted = vec![
            Granted {
                capability: flavour.capabilities[0].clone(),
                host: None,
            },
            Granted {
                capability: flavour.capabilities[1].clone(),
                host: Some("/dev/video2".into()),
            },
        ];
        let compose = build_app_compose(&AppCompose {
            flavour: &flavour,
            granted: &granted,
            instance_id: "lab-engine",
            device_id: Some("machine"),
            coord_server: "go.arkitekt.live",
            project: "engine",
            hub_network: Some("lab_internal"),
            mesh: None,
            engine: EngineKind::Docker,
            host_socket: Some("/run/user/1000/docker.sock"),
        });
        let app = &compose["services"]["app"];
        assert_eq!(
            app["image"],
            s("ghcr.io/arkitektio/plugin_engine@sha256:aaaa")
        );
        assert_eq!(app["hostname"], s("lab-engine"));
        let volumes: Vec<&str> = app["volumes"]
            .as_sequence()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(volumes.contains(&"/run/user/1000/docker.sock:/var/run/docker.sock"));
        assert_eq!(app["devices"][0], s("/dev/video2:/dev/video0"));
        // Not granted: no shared memory beyond the default.
        assert!(app.get("shm_size").is_none());
        assert_eq!(
            app["deploy"]["resources"]["reservations"]["devices"][0]["driver"],
            s("nvidia")
        );
        let env = &app["environment"];
        assert_eq!(env["ARKITEKT_INSTANCE_ID"], s("lab-engine"));
        assert_eq!(env["ARKITEKT_NETWORK"], s("lab_internal"));
        assert_eq!(env["DOCKER_HOST"], s("unix:///var/run/docker.sock"));
        assert_eq!(compose["networks"]["hub"]["external"], Value::Bool(true));
    }

    #[test]
    fn a_flavour_for_another_machine_is_passed_over_and_said_why() {
        let gpu = flavour(serde_json::json!([]), serde_json::json!([{"kind": "cuda"}]));
        assert_eq!(
            misfit(&gpu, &here()).as_deref(),
            Some("needs an NVIDIA GPU")
        );
        let mut release = Release {
            manifest: ReleaseManifest {
                identifier: "segmenter".into(),
                version: "1.0.0".into(),
                description: None,
                logo: None,
                scopes: vec![],
            },
            flavours: vec![Flavour {
                name: "gpu".into(),
                ..gpu
            }],
        };
        let refused = choose(&release, None, &here()).unwrap_err().to_string();
        assert!(refused.contains("`gpu` needs an NVIDIA GPU"), "{refused}");
        release.flavours.push(engine_flavour());
        assert_eq!(choose(&release, None, &here()).unwrap().name, "vanilla");
    }

    #[test]
    fn the_manifest_asks_for_what_the_flavour_requires() {
        let release = Release {
            manifest: ReleaseManifest {
                identifier: "plugin_engine".into(),
                version: "2.1.0".into(),
                description: Some("Runs apps".into()),
                logo: None,
                scopes: vec!["read".into(), "write".into()],
            },
            flavours: vec![],
        };
        let manifest = app_manifest(&release, &engine_flavour(), "lab", None, None);
        assert_eq!(manifest.identifier, "plugin_engine");
        assert_eq!(manifest.requirements[0].key, "kabinet");
        assert_eq!(manifest.instance_id, "lab");
    }
}
