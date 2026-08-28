use std::collections::HashSet;
use std::fs::canonicalize;
use std::sync::Mutex;

use konstruktor_core::connect::reachability;
use konstruktor_core::destroy::{self, DataPurge, Deletion, DeletionPlan};
use konstruktor_core::docker::{self, Container, DockerProbe};
use konstruktor_core::git::{self, Checkout, GitProbe};
use konstruktor_core::hosts;
use serde::{Deserialize, Serialize};
use tauri::command;
use tauri_plugin_fs::FsExt;

/// The Tauri command layer, and nothing else.
///
/// Every command here is a thin wrapper: the work lives in `konstruktor-core`, which the
/// CLI links against too. Anything that starts growing logic in this file belongs in the
/// core instead, or the two front ends will drift — which is the whole reason the core
/// exists.

/// The deployment folders this run of the app has started, so quitting can stop them
/// again.
///
/// Only what we started: a stack the user brought up from a terminal, or left running
/// from an earlier session, is none of our business to take down. That also means the
/// set is empty after a crash — an exit hook cannot cover a process that never exits.
#[derive(Default)]
pub struct StartedStacks(Mutex<HashSet<PathBuf>>);

impl StartedStacks {
    /// Both ends canonicalize, so the same folder reached two ways is one entry and
    /// `stop` actually finds what `up` inserted.
    fn key(path: &str) -> PathBuf {
        canonicalize(path).unwrap_or_else(|_| PathBuf::from(path))
    }

    pub fn started(&self, path: &str) {
        if let Ok(mut set) = self.0.lock() {
            set.insert(Self::key(path));
        }
    }

    pub fn stopped(&self, path: &str) {
        if let Ok(mut set) = self.0.lock() {
            set.remove(&Self::key(path));
        }
    }

    /// Empties the set and hands back what was in it — the exit hook runs once.
    pub fn take(&self) -> Vec<PathBuf> {
        match self.0.lock() {
            Ok(mut set) => set.drain().collect(),
            Err(_) => Vec::new(),
        }
    }
}

#[command]
pub async fn probe_docker() -> DockerProbe {
    docker::probe().await
}

/// Git, kept apart from the Docker probe on purpose: the two have different verdicts and
/// different consequences. Docker missing stops a deployment; git missing only takes the
/// dev-hub option away.
#[command]
pub async fn probe_git() -> GitProbe {
    git::probe()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContainerQuery {
    containers: Vec<Container>,
}

#[command]
pub async fn list_deployment_containers(path: String) -> Result<ContainerQuery, String> {
    docker::list_deployment_containers(&path)
        .await
        .map(|containers| ContainerQuery { containers })
}

/// What the local daemon holds for every image this deployment's stack declares.
///
/// Paired with the running containers' `image_id`, this is how the dashboard tells the
/// three states apart: never pulled, pulled and running, and pulled but still waiting for
/// a restart. It answers nothing about the registry — see `docker::image_states`.
#[command]
pub async fn deployment_images(path: String) -> Result<Vec<docker::ImageState>, String> {
    let profile = profile::read_profile(&PathBuf::from(path)).map_err(|e| e.to_string())?;
    docker::image_states(&profile.config.stack_images()).await
}

#[command]
pub async fn restart_container(container_id: String) -> Result<(), String> {
    docker::restart_container(&container_id).await
}

/// The addresses worth advertising, classified and ordered, with the reach presets
/// already resolved against them.
///
/// The classification used to live in `src/connect/hosts.ts`, applied to the raw list
/// this command's predecessor returned. It sits next to the enumeration now, so the CLI
/// gets the same answer without reimplementing the rules — and the presets ship with it
/// for the same reason, so the wizard and `--reach` cannot drift apart.
#[command]
pub async fn host_candidates(
    mesh_domain: Option<String>,
    mesh_hostname: Option<String>,
) -> Result<hosts::HostDiscovery, String> {
    // Whatever the caller could find out about this hub's tailnet. Without it every
    // tailnet address on the machine is somebody else's — which, before the hub has
    // joined anything, is exactly true.
    let mesh = hosts::KnownMesh {
        domain: mesh_domain.filter(|d| !d.trim().is_empty()),
        hostname: mesh_hostname.filter(|h| !h.trim().is_empty()),
    };
    let candidates = hosts::host_candidates(&hosts::bindings().await?, &mesh);
    let presets = hosts::reach_presets(&candidates);
    Ok(hosts::HostDiscovery {
        candidates,
        presets,
    })
}

/// What address the internet sees this machine as.
///
/// The endpoint comes from the frontend's settings, and there is no default: this is the
/// only request konstruktor makes to a host the user did not name, so it happens only
/// when they have said which one.
#[command]
pub async fn egress_identity(endpoint: String) -> Result<String, String> {
    reachability::egress_identity(&endpoint)
        .await
        .map(|identity| identity.address)
        .map_err(|e| e.to_string())
}

/// Asks a configured prober to connect back to one advertised address.
#[command]
pub async fn probe_reachability(
    prober: String,
    host: String,
    port: u16,
    ssl: bool,
) -> Result<reachability::ProbeResult, String> {
    Ok(reachability::probe(&prober, &reachability::probe_url(&host, port, ssl)).await)
}

/// Resolve a path the user picked to its canonical form, so the registry and the compose
/// working-dir label always agree (symlinks, `..`, trailing slashes).
#[command]
pub fn canonicalize_path(path: String) -> Result<String, String> {
    canonicalize(&path)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PreparedDir {
    path: String,
    created: bool,
}

/// Creates a deployment folder and grants the app access to it, in that order.
///
/// Deployments live in folders the user chooses, which the static filesystem scope in
/// `capabilities/` cannot know about; granting them at runtime keeps that scope tight
/// instead of widening it to the whole filesystem. Returns whether this call is what
/// brought the folder into existence — the caller deletes it again if it turns out to be
/// unusable.
#[command]
pub fn prepare_deployment_dir(app: tauri::AppHandle, path: String) -> Result<PreparedDir, String> {
    let created = !std::path::Path::new(&path).exists();
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;

    let dir = canonicalize(&path).map_err(|e| e.to_string())?;
    app.fs_scope()
        .allow_directory(&dir, true)
        .map_err(|e| e.to_string())?;

    Ok(PreparedDir {
        path: dir.to_string_lossy().to_string(),
        created,
    })
}

/// Removes a folder this app created and then found it could not use. Refuses anything
/// that is not empty, so it can never take a deployment with it.
#[command]
pub fn discard_empty_dir(path: String) -> Result<(), String> {
    let mut entries = std::fs::read_dir(&path).map_err(|e| e.to_string())?;
    if entries.next().is_some() {
        return Err("The folder is not empty".to_string());
    }
    std::fs::remove_dir(&path).map_err(|e| e.to_string())
}

#[command]
pub fn allow_deployment_dir(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let dir = canonicalize(&path).map_err(|e| e.to_string())?;
    app.fs_scope()
        .allow_directory(&dir, true)
        .map_err(|e| e.to_string())
}

// --- the deployment surface -------------------------------------------------
//
// Everything below is the same code the `konstruktor` CLI runs. The frontend collects
// answers and calls these; it does not generate, authorize or write anything itself.

use konstruktor_core::connect::wellknown::{self, WellKnownFakts};
use konstruktor_core::create::{self, CreateEvent, HubAnswers};
use konstruktor_core::generate::{generate_hub_files, IssuedIdentity};
use konstruktor_core::registry::{self, DeploymentRecord};
use konstruktor_core::{compose, config, credentials, profile};
use std::path::PathBuf;
use tauri::ipc::Channel;

/// Create a hub, streaming progress back as it goes.
///
/// The device code appears in `CreateEvent::Staged`, so the progress dialog shows it —
/// which is why this can be one call rather than a wizard step that can go stale.
#[command]
pub async fn create_hub(
    started: tauri::State<'_, StartedStacks>,
    answers: HubAnswers,
    on_event: Channel<CreateEvent>,
) -> Result<String, String> {
    let cancel = tokio_util::sync::CancellationToken::new();

    let created = create::create_hub(&answers, &cancel, &move |event| {
        // A closed channel means the window went away; the creation still finishes.
        let _ = on_event.send(event);
    })
    .await
    .map_err(|e| e.to_string())?;

    let path = created.path.to_string_lossy().to_string();
    // The wizard starts the stack as part of creating it, which counts the same as
    // pressing Start would.
    if answers.start {
        started.started(&path);
    }

    Ok(path)
}

/// Create a plugin engine: one deployer container with the Docker socket, in its own
/// folder and its own compose project.
///
/// Streams the same `CreateEvent`s a hub does — device code included, since an engine is
/// authorized the same way, through the app flow rather than the hub one — so the
/// progress dialog is shared.
#[command]
pub async fn create_engine(
    started: tauri::State<'_, StartedStacks>,
    answers: konstruktor_core::engine::EngineAnswers,
    on_event: Channel<CreateEvent>,
) -> Result<String, String> {
    let start = answers.start;
    let cancel = tokio_util::sync::CancellationToken::new();
    let created = konstruktor_core::engine::create_engine(&answers, &cancel, &move |event| {
        let _ = on_event.send(event);
    })
    .await
    .map_err(|e| e.to_string())?;

    let path = created.path.to_string_lossy().to_string();
    if start {
        started.started(&path);
    }
    Ok(path)
}

/// The files a set of answers would produce, for the summary step's "no surprises" list.
///
/// Generated from a throwaway config: this is a preview, and the profile that actually
/// gets written is minted once, inside `create_hub`.
#[command]
pub fn preview_hub_files(answers: HubAnswers) -> Vec<String> {
    let config = config::hub::build_hub_config(&config::hub::HubConfigOptions {
        coord_server: answers.coord_server.clone(),
        rekuest_server: answers.rekuest_server.clone(),
        services: Some(answers.services.clone()),
        http_port: Some(answers.http_port),
        https_port: Some(answers.https_port),
        ssl: answers.ssl,
        // The bind mounts a source checkout adds live in the compose file, so the
        // preview only tells the truth if it knows which services asked for one.
        dev_hub: answers.dev_hub,
        service_options: answers.service_options.clone(),
        ..Default::default()
    });
    generate_hub_files(&config, &IssuedIdentity::default())
        .into_keys()
        .collect()
}

#[command]
pub async fn discover_server(server: String) -> Result<WellKnownFakts, String> {
    wellknown::discover(&server)
        .await
        .map_err(|e| e.to_string())
}

/// The tailnet a coordination server runs, if it declares one.
///
/// Needed to tell an address on *this hub's* mesh from one on whatever other tailnet the
/// machine is already on. Absent — which is every server today — the address step says
/// "other tailscale" rather than guessing, so this failing is not an error.
#[command]
pub async fn mesh_domain(server: String) -> Result<Option<String>, String> {
    Ok(wellknown::discover(&server)
        .await
        .ok()
        .and_then(|fakts| fakts.mesh_domain()))
}

#[command]
pub fn suggest_folder(base: Option<String>) -> Option<String> {
    // The name the folder is offered under follows what is being created — `MyEngine`
    // for a plugin engine — so nobody is handed a folder called MyHub for something
    // that is not one.
    let base = base
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .unwrap_or("MyHub");
    create::suggest_folder(base).map(|p| p.to_string_lossy().to_string())
}

#[command]
pub fn identifier_from_folder(path: String) -> String {
    create::identifier_from_folder(&PathBuf::from(path))
}

/// Whether a deployment can be created in a folder, and why not when it cannot.
#[derive(Debug, Serialize)]
pub struct FolderReport {
    ok: bool,
    message: String,
}

#[command]
pub fn inspect_folder(path: String) -> FolderReport {
    let verdict = registry::inspect_folder(&registry::load(), &PathBuf::from(path));
    FolderReport {
        ok: verdict.can_create(),
        message: verdict.describe(),
    }
}

#[command]
pub fn list_deployments() -> Vec<DeploymentRecord> {
    registry::load().deployments
}

#[command]
pub fn forget_deployment(id: String) -> Result<(), String> {
    let mut store = registry::load();
    store.deployments.retain(|d| d.id != id);
    registry::save(&store).map_err(|e| e.to_string())
}

/// What deleting a deployment would take with it, so the dialog can say so before asking.
#[command]
pub fn plan_deletion(id: String) -> Result<DeletionPlan, String> {
    let store = registry::load();
    let record = store
        .deployments
        .iter()
        .find(|d| d.id == id)
        .ok_or_else(|| destroy::DeleteError::UnknownDeployment.to_string())?;
    destroy::plan(record)
        .map(|(_, plan)| plan)
        .map_err(|e| e.to_string())
}

/// Deletes a deployment and everything it put on this machine.
///
/// By id, never by path: the folder is resolved from the registry inside the core, so no
/// caller can name an arbitrary directory to be removed recursively. The sequence and its
/// guards live in `konstruktor_core::destroy`; this only hands the result back and stops
/// the exit hook from trying to take down a folder that is no longer there.
///
/// Off the main thread. A delete is `docker compose down --volumes --remove-orphans`
/// followed by a recursive removal of the folder — seconds at best, and a great deal
/// longer for a dev hub with checkouts in it. A synchronous command runs on the thread
/// that draws the window, so the whole app, including the dialog's own "Deleting…",
/// froze for the duration and looked like a hang.
#[command]
pub async fn delete_deployment(
    started: tauri::State<'_, StartedStacks>,
    id: String,
) -> Result<Deletion, String> {
    let deleted = tauri::async_runtime::spawn_blocking(move || destroy::delete(&id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    started.stopped(&deleted.path);
    Ok(deleted)
}

/// Deletes a hub's data and leaves the hub itself standing.
///
/// By id, for the same reason `delete_deployment` is: the folder and the data directories
/// inside it are resolved and guarded in the core, so no caller can name a directory to be
/// removed recursively.
///
/// This is the *only* path in the app that destroys data. `docker compose down --volumes`
/// is not — the stack keeps its database in a bind mount and declares no named volumes, so
/// that command removes nothing.
/// Blocking, and so run off the main thread for the reason `delete_deployment` is.
#[command]
pub async fn purge_deployment_data(
    started: tauri::State<'_, StartedStacks>,
    id: String,
) -> Result<DataPurge, String> {
    let purged = tauri::async_runtime::spawn_blocking(move || destroy::purge_data(&id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    // The purge took the stack down with it, so the exit hook has nothing left to stop.
    started.stopped(&purged.path);
    Ok(purged)
}

/// One service, as the dashboard lists it.
#[derive(Debug, Serialize)]
pub struct ServiceView {
    id: String,
    name: String,
    host: String,
    /// Where a browser reaches it through the gateway.
    url: String,
    /// The image the profile pins this service to, e.g. `jhnnsrs/rekuest:next`.
    image: Option<String>,
    /// That image's tag on its own — the service's release channel.
    tag: Option<String>,
}

/// The release channel a hub follows, read off the images its services are pinned to.
///
/// There is no channel field in the profile: the channel *is* the set of tags, and those
/// are per-service. A hub whose services carry different tags has no single channel, and
/// saying so is the point of `tags` — the alternative is a UI that picks one and lies.
#[derive(Debug, Serialize)]
pub struct ChannelView {
    /// The one tag every service shares, when they do share one.
    tag: Option<String>,
    /// Every distinct tag in play, sorted. More than one means the hub is mixed.
    tags: Vec<String>,
}

/// The tag out of an image reference, tolerating a registry host with a port
/// (`registry:5000/image:tag`) by only looking after the last slash.
fn image_tag(image: &str) -> Option<String> {
    let last = image.rsplit('/').next().unwrap_or(image);
    last.rsplit_once(':').map(|(_, tag)| tag.to_string())
}

/// Everything the dashboard reads out of a deployment folder, derived once here so the
/// frontend does no URL-building of its own.
#[derive(Debug, Serialize)]
pub struct HubStatus {
    profile: profile::Profile,
    authorized: bool,
    identifier: Option<String>,
    authorized_at: Option<String>,
    /// The gateway's own address, and the admin account for every service's admin panel.
    gateway_url: String,
    admin_user: String,
    admin_password: String,
    services: Vec<ServiceView>,
    mesh_hostname: Option<String>,
    /// The port an alias advertises, as the manifest computes it — so a reachability
    /// probe aims at the same socket the coordination server would hand out.
    advertised_port: u16,
    /// What this hub last told the coordination server it was reachable at.
    ///
    /// The authorize screen seeds from this rather than from a fresh scan: it exists to
    /// *add* the tailnet address, and a scan of this machine will never find one.
    advertised_hosts: Vec<konstruktor_core::connect::manifest::AdvertisedHost>,
    /// The release channel the enabled services are pinned to.
    channel: ChannelView,
}

#[command]
pub fn hub_status(path: String) -> Result<HubStatus, String> {
    let dir = PathBuf::from(path);
    let profile = profile::read_profile(&dir).map_err(|e| e.to_string())?;
    let creds = credentials::read_credentials(&dir);
    let config = &profile.config;

    let scheme = if config.gateway.ssl { "https" } else { "http" };
    let port = konstruktor_core::connect::manifest::advertised_port(config);
    let host = config.domain.clone().unwrap_or_else(|| "localhost".into());

    // A default port is left off the URL; anything else has to be spelled out.
    let default_port = if config.gateway.ssl { 443 } else { 80 };
    let authority = if port == default_port {
        host.clone()
    } else {
        format!("{host}:{port}")
    };
    let gateway_url = format!("{scheme}://{authority}");

    let catalog = konstruktor_core::catalog::catalog();
    let services = config
        .enabled_services()
        .into_iter()
        .map(|id| {
            let block = config.service(id);
            let meta = catalog.iter().find(|m| m.id == id);
            ServiceView {
                id: id.as_str().to_string(),
                name: meta
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| id.as_str().into()),
                host: block.host.clone(),
                url: format!("{gateway_url}/{}", block.host),
                image: block.image.clone(),
                tag: block.image.as_deref().and_then(image_tag),
            }
        })
        .collect::<Vec<ServiceView>>();

    let mut tags: Vec<String> = services.iter().filter_map(|s| s.tag.clone()).collect();
    tags.sort();
    tags.dedup();
    let channel = ChannelView {
        tag: if tags.len() == 1 {
            tags.first().cloned()
        } else {
            None
        },
        tags,
    };

    Ok(HubStatus {
        authorized: creds.is_some(),
        identifier: creds.as_ref().map(|c| c.identifier.clone()),
        authorized_at: creds.as_ref().map(|c| c.authorized_at.clone()),
        gateway_url,
        admin_user: config.global_admin.clone(),
        admin_password: config.global_admin_password.clone(),
        services,
        channel,
        mesh_hostname: config
            .mesh
            .as_ref()
            .filter(|m| m.enabled)
            .map(|m| m.hostname.clone()),
        advertised_port: port,
        advertised_hosts: creds
            .as_ref()
            .map(|c| c.advertised_hosts.clone())
            .unwrap_or_default(),
        profile,
    })
}

/// The source checkouts a dev hub keeps under `mounts/`.
///
/// An empty list is the answer for every ordinary hub, so the dashboard needs no separate
/// "is this a dev hub" question — there is either something to switch branches in or
/// there is not.
#[command]
pub fn deployment_checkouts(path: String) -> Result<Vec<Checkout>, String> {
    let dir = PathBuf::from(path);
    let profile = profile::read_profile(&dir).map_err(|e| e.to_string())?;
    Ok(git::checkouts(&dir, &profile.config))
}

/// The branches one checkout could switch to. Fetches first, so it is current.
#[command]
pub async fn checkout_branches(path: String, service: String) -> Result<Vec<String>, String> {
    let dir = PathBuf::from(path);
    git::branches(&git::checkout_dir(&dir, &service))
}

/// Puts one checkout on another branch.
///
/// The container keeps running whatever it loaded until it is recreated — the caller is
/// expected to say so, and the dashboard offers the restart next to this.
#[command]
pub async fn switch_checkout_branch(
    path: String,
    service: String,
    branch: String,
) -> Result<Checkout, String> {
    let dir = PathBuf::from(path);
    let at = git::checkout_dir(&dir, &service);
    git::switch_branch(&service, &at, &branch).map_err(|e| e.to_string())?;

    // The fresh state rather than a bare ok: the caller has to re-render the card, and
    // reading it here saves a second round trip that could disagree with this one.
    let profile = profile::read_profile(&dir).map_err(|e| e.to_string())?;
    let repo = profile
        .config
        .enabled_services()
        .into_iter()
        .map(|id| profile.config.service(id))
        .find(|s| s.host == service)
        .map(|s| s.github_repo.clone())
        .unwrap_or_default();

    Ok(git::read_checkout(&service, &repo, &at))
}

/// The services a picker can offer, with their display copy.
#[command]
pub fn service_catalog() -> Vec<konstruktor_core::catalog::ServiceMeta> {
    konstruktor_core::catalog::catalog()
}

/// Makes a Django superuser in one service, after the fact.
///
/// Not part of creating a hub: the account has to be made in a container that is running,
/// against a database that exists, and it is per service — so it belongs on the dashboard
/// next to the service it is for, not in a wizard step asked before anything is up.
#[command]
pub async fn create_superuser(
    path: String,
    service: String,
    username: String,
    password: String,
    email: Option<String>,
) -> Result<String, String> {
    let args = compose::create_superuser(
        &service,
        &username,
        &password,
        email.as_deref().map(str::trim).filter(|e| !e.is_empty()),
    );

    let output = std::process::Command::new("docker")
        .args(&args)
        .current_dir(&path)
        .output()
        .map_err(|e| e.to_string())?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        // Django says why on stderr — "that username is already taken", most often, which
        // is a thing the user needs to read rather than a generic failure.
        Err(format!(
            "{}{}",
            stdout,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

/// Runs one `docker compose` subcommand in a deployment folder.
#[command]
pub async fn compose_command(
    started: tauri::State<'_, StartedStacks>,
    path: String,
    action: String,
    service: Option<String>,
    tail: Option<u32>,
) -> Result<String, String> {
    let args: Vec<String> = match action.as_str() {
        "up" => compose::up().into_iter().map(String::from).collect(),
        "stop" => compose::stop().into_iter().map(String::from).collect(),
        "down" => compose::down().into_iter().map(String::from).collect(),
        "pull" => compose::pull().into_iter().map(String::from).collect(),
        "ps" => compose::ps().into_iter().map(String::from).collect(),
        "logs" => compose::logs(service.as_deref(), tail.unwrap_or(200)),
        other => return Err(format!("unknown compose action `{other}`")),
    };

    let output = std::process::Command::new("docker")
        .args(&args)
        .current_dir(&path)
        .output()
        .map_err(|e| e.to_string())?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if output.status.success() {
        // Remember what we brought up, and forget it again the moment the user takes it
        // down themselves — otherwise quitting would stop a stack a second time.
        match action.as_str() {
            "up" => started.started(&path),
            "stop" | "down" => started.stopped(&path),
            _ => {}
        }
        Ok(stdout)
    } else {
        // Compose writes its progress to stderr, so a failure's explanation is there.
        Err(format!(
            "{}{}",
            stdout,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

/// Re-authorize a hub that already exists — what the connect screen does.
#[command]
pub async fn reauthorize_hub(
    path: String,
    coord_server: String,
    identifier: String,
    description: Option<String>,
    hosts: Vec<konstruktor_core::connect::manifest::AdvertisedHost>,
    reachable_hosts: Vec<String>,
    request_auth_key: bool,
    on_event: Channel<CreateEvent>,
) -> Result<(), String> {
    let cancel = tokio_util::sync::CancellationToken::new();

    create::reauthorize(
        &create::ReauthorizeAnswers {
            dir: PathBuf::from(path),
            coord_server,
            identifier,
            description,
            hosts,
            reachable_hosts,
            request_auth_key,
        },
        &cancel,
        &move |event| {
            let _ = on_event.send(event);
        },
    )
    .await
    .map(|_| ())
    .map_err(|e| e.to_string())
}
