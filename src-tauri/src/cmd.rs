use std::collections::HashSet;
use std::fs::canonicalize;
use std::sync::Mutex;

use konstruktor_core::docker::{self, Container, DockerProbe};
use konstruktor_core::hosts::{self, Binding, HostCandidate};
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

#[command]
pub async fn restart_container(container_id: String) -> Result<(), String> {
    docker::restart_container(&container_id).await
}

#[command]
pub async fn list_network_interfaces(v4: bool) -> Result<Vec<Binding>, String> {
    // IPv4 only, as it always has been; the flag is kept so the frontend call site does
    // not have to change while the port is in flight.
    let _ = v4;
    hosts::bindings().await
}

/// The addresses worth advertising, already classified and ordered.
///
/// The classification used to live in `src/connect/hosts.ts`, applied to the raw list
/// this command's predecessor returned. It sits next to the enumeration now, so the CLI
/// gets the same answer without reimplementing the rules.
#[command]
pub async fn host_candidates() -> Result<Vec<HostCandidate>, String> {
    Ok(hosts::host_candidates(&hosts::bindings().await?))
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
        ..Default::default()
    });
    generate_hub_files(&config, &IssuedIdentity::default())
        .into_keys()
        .collect()
}

#[command]
pub async fn discover_server(server: String) -> Result<WellKnownFakts, String> {
    wellknown::discover(&server).await.map_err(|e| e.to_string())
}

#[command]
pub fn suggest_folder() -> Option<String> {
    create::suggest_folder("MyHub").map(|p| p.to_string_lossy().to_string())
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

/// One service, as the dashboard lists it.
#[derive(Debug, Serialize)]
pub struct ServiceView {
    id: String,
    name: String,
    host: String,
    /// Where a browser reaches it through the gateway.
    url: String,
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
                name: meta.map(|m| m.name.clone()).unwrap_or_else(|| id.as_str().into()),
                host: block.host.clone(),
                url: format!("{gateway_url}/{}", block.host),
            }
        })
        .collect();

    Ok(HubStatus {
        authorized: creds.is_some(),
        identifier: creds.as_ref().map(|c| c.identifier.clone()),
        authorized_at: creds.as_ref().map(|c| c.authorized_at.clone()),
        gateway_url,
        admin_user: config.global_admin.clone(),
        admin_password: config.global_admin_password.clone(),
        services,
        mesh_hostname: config
            .mesh
            .as_ref()
            .filter(|m| m.enabled)
            .map(|m| m.hostname.clone()),
        profile,
    })
}

/// The services a picker can offer, with their display copy.
#[command]
pub fn service_catalog() -> Vec<konstruktor_core::catalog::ServiceMeta> {
    konstruktor_core::catalog::catalog()
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
        "down-volumes" => compose::down_volumes().into_iter().map(String::from).collect(),
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
            "stop" | "down" | "down-volumes" => started.stopped(&path),
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
