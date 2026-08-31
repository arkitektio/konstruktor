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
    konstruktor_core::updates::images_for_deployment(&PathBuf::from(path)).await
}

/// Asks each image's registry whether its tag has moved on since the last pull.
///
/// Network, not the engine — see `konstruktor_core::updates`. The dashboard runs it once
/// when it opens, off to the side, and only says "update" when the answer is yes.
#[command]
pub async fn check_updates(
    path: String,
) -> Result<Vec<konstruktor_core::updates::UpstreamCheck>, String> {
    konstruktor_core::updates::for_deployment(&PathBuf::from(path)).await
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
use konstruktor_core::registry::{self, DeploymentRecord};
use konstruktor_core::{backup, compose, compose_file, profile, restore};
use std::path::PathBuf;
use tauri::ipc::Channel;

/// The authorization that is running, if one is, so a Cancel button has something to
/// pull.
///
/// Creating a hub, creating an engine and re-authorizing all wait on a person accepting
/// a device code somewhere else, which can take as long as they take — and until this,
/// the only way out was to quit the app. One slot for all three: they are all started
/// from a screen that fills the window, so a second cannot be running behind the first.
///
/// Only the wait is interruptible — see `authorize::wait_for_hub`, which is where the
/// token is read. Cancelling during the Docker probe or while files are being written
/// takes effect at the next wait, or not at all.
#[derive(Default)]
pub struct AuthorizeState(Mutex<Option<CancellationToken>>);

impl AuthorizeState {
    fn begin(&self) -> CancellationToken {
        let token = CancellationToken::new();
        if let Ok(mut slot) = self.0.lock() {
            // Whatever was there is finished or abandoned; a stale token left behind by
            // a panicking call must never make the next attempt uncancellable.
            *slot = Some(token.clone());
        }
        token
    }

    /// Clears the slot, whether the run succeeded, failed or was cancelled — a token
    /// left in it would be pulled by the *next* run's Cancel button.
    fn end(&self) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = None;
        }
    }

    fn cancel(&self) {
        if let Ok(slot) = self.0.lock() {
            if let Some(token) = slot.as_ref() {
                token.cancel();
            }
        }
    }
}

/// Stop waiting for the device code to be accepted. The call that was waiting returns an
/// error saying it was cancelled; nothing has been written by then.
#[command]
pub async fn cancel_authorization(state: tauri::State<'_, AuthorizeState>) -> Result<(), String> {
    state.cancel();
    Ok(())
}

/// Create a hub, streaming progress back as it goes.
///
/// The device code appears in `CreateEvent::Staged`, so the progress step shows it —
/// which is why this can be one call rather than a wizard step that can go stale.
#[command]
pub async fn create_hub(
    app: tauri::AppHandle,
    started: tauri::State<'_, StartedStacks>,
    authorizing: tauri::State<'_, AuthorizeState>,
    answers: HubAnswers,
    on_event: Channel<CreateEvent>,
) -> Result<String, String> {
    let cancel = authorizing.begin();

    let created = create::create_hub(&answers, &cancel, &move |event| {
        // A closed channel means the window went away; the creation still finishes.
        let _ = on_event.send(event);
    })
    .await;
    // Before the `?`: an early return with the token still in the slot would leave the
    // next attempt with a Cancel button wired to a call that is already over.
    authorizing.end();
    let created = created.map_err(|e| e.to_string())?;

    let path = created.path.to_string_lossy().to_string();
    // The wizard starts the stack as part of creating it, which counts the same as
    // pressing Start would.
    if answers.start {
        started.started(&path);
    }
    crate::tray::poke(&app);

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
    app: tauri::AppHandle,
    started: tauri::State<'_, StartedStacks>,
    authorizing: tauri::State<'_, AuthorizeState>,
    answers: konstruktor_core::engine::EngineAnswers,
    on_event: Channel<CreateEvent>,
) -> Result<String, String> {
    let start = answers.start;
    let cancel = authorizing.begin();
    let created = konstruktor_core::engine::create_engine(&answers, &cancel, &move |event| {
        let _ = on_event.send(event);
    })
    .await;
    authorizing.end();
    let created = created.map_err(|e| e.to_string())?;

    let path = created.path.to_string_lossy().to_string();
    if start {
        started.started(&path);
    }
    crate::tray::poke(&app);
    Ok(path)
}

/// The files a set of answers would produce, for the summary step's "no surprises" list.
///
/// Generated from a throwaway config: this is a preview, and the profile that actually
/// gets written is minted once, inside `create_hub`.
#[command]
pub fn preview_hub_files(answers: HubAnswers) -> Vec<String> {
    create::preview_files(&answers)
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
pub fn forget_deployment(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let mut store = registry::load();
    store.deployments.retain(|d| d.id != id);
    registry::save(&store).map_err(|e| e.to_string())?;
    crate::tray::poke(&app);
    Ok(())
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
    app: tauri::AppHandle,
    started: tauri::State<'_, StartedStacks>,
    id: String,
) -> Result<Deletion, String> {
    let deleted = tauri::async_runtime::spawn_blocking(move || destroy::delete(&id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    started.stopped(&deleted.path);
    crate::tray::poke(&app);
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
    app: tauri::AppHandle,
    started: tauri::State<'_, StartedStacks>,
    id: String,
) -> Result<DataPurge, String> {
    let purged = tauri::async_runtime::spawn_blocking(move || destroy::purge_data(&id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    // The purge took the stack down with it, so the exit hook has nothing left to stop.
    started.stopped(&purged.path);
    // Every other state-changing command pokes the tray; this one did not, so the menu
    // went on showing the hub as running until the next ten-second tick.
    crate::tray::poke(&app);
    Ok(purged)
}

/// The dashboard's view of a deployment folder. Both of these are `konstruktor-core`
/// types — the derivations behind them (gateway URL, service list, release channel) moved
/// there so `konstruktor status` answers from the same code. The names are re-exported
/// unchanged because `src/api/types.ts` reads these keys.
pub use konstruktor_core::status::HubView as HubStatus;

#[command]
pub fn hub_status(path: String) -> Result<HubStatus, String> {
    konstruktor_core::status::hub_view(&PathBuf::from(path)).map_err(|e| e.to_string())
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

    let output = konstruktor_core::docker::command()
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

/// One line of a compose command's output, as it is written.
///
/// Compose narrates on stderr — `Container hub-db-1  Starting`, then `Started` — and that
/// narration is what a button can turn into progress. Sent with the ANSI stripped, and
/// with `--ansi never` asked for too, since one of the two is not always enough.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeLine {
    pub line: String,
    pub stderr: bool,
}

/// `compose_command`, streaming its output over `on_line` while it runs.
///
/// The same bookkeeping as the buffered one: the started set and the tray are updated on
/// success, and a failure carries compose's own explanation. Callers that want to *show*
/// what is happening use this; the buffered one stays for `ps` and `logs`, whose whole
/// output is the answer.
#[command]
pub async fn compose_command_streamed(
    app: tauri::AppHandle,
    started: tauri::State<'_, StartedStacks>,
    path: String,
    action: String,
    on_line: Channel<ComposeLine>,
) -> Result<String, String> {
    let args = compose_args(&action, None, None)?;
    let stdout = run_streamed(args, &path, &on_line).await?;

    match action.as_str() {
        "up" => started.started(&path),
        "stop" | "down" => started.stopped(&path),
        _ => {}
    }
    crate::tray::poke(&app);
    Ok(stdout)
}

/// Bring one service up to date: fetch its image if asked, then recreate that container
/// and nothing else.
///
/// `pull` is a decision, not a formality. An image that has already been fetched is on
/// this machine and applying it must work with the registry unreachable — so the button
/// that only recreates says so by passing `false`, rather than being made to go to the
/// network for an answer it does not need.
#[command]
pub async fn update_service(
    app: tauri::AppHandle,
    started: tauri::State<'_, StartedStacks>,
    path: String,
    service: String,
    pull: bool,
    on_line: Channel<ComposeLine>,
) -> Result<String, String> {
    let mut output = String::new();
    if pull {
        output.push_str(&run_streamed(compose::pull_service(&service), &path, &on_line).await?);
    }
    output.push_str(&run_streamed(compose::up_service(&service), &path, &on_line).await?);

    // This brought a container up, so the same bookkeeping a whole-stack `up` does.
    started.started(&path);
    crate::tray::poke(&app);
    Ok(output)
}

/// Runs one compose invocation, streaming both its streams over `on_line` as they are
/// written and returning what it put on stdout.
///
/// Takes the channel by reference so a command made of two invocations — pull, then up —
/// can narrate both through the one channel.
async fn run_streamed(
    mut args: Vec<String>,
    path: &str,
    on_line: &Channel<ComposeLine>,
) -> Result<String, String> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    // Plain, line-by-line narration. Without a TTY compose already avoids the redrawing
    // progress UI; `--ansi never` also keeps colour codes out of the lines.
    args.splice(1..1, ["--ansi".to_string(), "never".to_string()]);

    let engine = konstruktor_core::engine_probe::engine();
    let mut child = engine
        .async_command()
        .args(&args)
        .current_dir(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;

    let out_channel = on_line.clone();
    let err_channel = on_line.clone();
    let out_task = tauri::async_runtime::spawn(async move {
        let mut collected = String::new();
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(raw)) = lines.next_line().await {
            let line = clean(&raw);
            collected.push_str(&line);
            collected.push('\n');
            let _ = out_channel.send(ComposeLine { line, stderr: false });
        }
        collected
    });
    let err_task = tauri::async_runtime::spawn(async move {
        let mut collected = String::new();
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(raw)) = lines.next_line().await {
            let line = clean(&raw);
            if line.trim().is_empty() {
                continue;
            }
            collected.push_str(&line);
            collected.push('\n');
            let _ = err_channel.send(ComposeLine { line, stderr: true });
        }
        collected
    });

    let status = child.wait().await.map_err(|e| e.to_string())?;
    let stdout = out_task.await.unwrap_or_default();
    let stderr = err_task.await.unwrap_or_default();

    if status.success() {
        Ok(stdout)
    } else {
        Err(format!("{stdout}{stderr}"))
    }
}

fn clean(raw: &str) -> String {
    String::from_utf8_lossy(&strip_ansi_escapes::strip(raw)).into_owned()
}

fn compose_args(
    action: &str,
    service: Option<&str>,
    tail: Option<u32>,
) -> Result<Vec<String>, String> {
    Ok(match action {
        "up" => compose::up().into_iter().map(String::from).collect(),
        "stop" => compose::stop().into_iter().map(String::from).collect(),
        "down" => compose::down().into_iter().map(String::from).collect(),
        "pull" => compose::pull().into_iter().map(String::from).collect(),
        "ps" => compose::ps().into_iter().map(String::from).collect(),
        "logs" => compose::logs(service, tail.unwrap_or(200)),
        other => return Err(format!("unknown compose action `{other}`")),
    })
}

/// Runs one `docker compose` subcommand in a deployment folder.
#[command]
pub async fn compose_command(
    app: tauri::AppHandle,
    started: tauri::State<'_, StartedStacks>,
    path: String,
    action: String,
    service: Option<String>,
    tail: Option<u32>,
) -> Result<String, String> {
    let args = compose_args(&action, service.as_deref(), tail)?;

    let output = konstruktor_core::docker::command()
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
        crate::tray::poke(&app);
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
    authorizing: tauri::State<'_, AuthorizeState>,
    path: String,
    coord_server: String,
    identifier: String,
    description: Option<String>,
    hosts: Vec<konstruktor_core::connect::manifest::AdvertisedHost>,
    reachable_hosts: Vec<String>,
    request_auth_key: bool,
    on_event: Channel<CreateEvent>,
) -> Result<(), String> {
    let cancel = authorizing.begin();

    let done = create::reauthorize(
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
    .await;
    authorizing.end();
    done.map(|_| ()).map_err(|e| e.to_string())
}


// --- the compose file, by hand ---------------------------------------------

/// The compose file and whether a previous version is there to go back to.
#[derive(Debug, Serialize)]
pub struct ComposeFileView {
    contents: String,
    /// What the generator would write from the profile today — the "reset" target.
    generated: String,
    has_backup: bool,
}

#[command]
pub fn read_compose_file(path: String) -> Result<ComposeFileView, String> {
    let dir = PathBuf::from(path);
    Ok(ComposeFileView {
        contents: compose_file::read(&dir).map_err(|e| e.to_string())?,
        // An engine has no profile; its editor simply has nothing to reset to.
        generated: compose_file::regenerate(&dir).unwrap_or_default(),
        has_backup: compose_file::has_backup(&dir),
    })
}

#[command]
pub fn read_compose_backup(path: String) -> Result<Option<String>, String> {
    compose_file::read_backup(&PathBuf::from(path)).map_err(|e| e.to_string())
}

/// Writes the file, keeping the previous one as `docker-compose.yaml.bak`. Refuses
/// anything that is not YAML with a `services:` mapping — see the core for why.
#[command]
pub fn write_compose_file(path: String, contents: String) -> Result<(), String> {
    compose_file::write(&PathBuf::from(path), &contents).map_err(|e| e.to_string())
}

/// Docker's own verdict on the file on disk: `None` when it accepts it, otherwise what
/// it printed. An `Err` means the engine could not be asked at all.
#[command]
pub async fn validate_compose_file(path: String) -> Result<Option<String>, String> {
    match compose_file::validate(&PathBuf::from(path)).await {
        Ok(()) => Ok(None),
        Err(problem) if problem.contains("executable file not found") => Err(problem),
        Err(problem) => Ok(Some(problem)),
    }
}

// --- backups -------------------------------------------------------------------

/// Where a backup started now would land, for the dialog to show before it starts.
#[command]
pub fn backup_folder(path: String, target: String) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    backup::backup_folder(
        &backup::BackupRequest {
            dir: PathBuf::from(path),
            target: PathBuf::from(target),
        },
        now,
    )
    .display()
    .to_string()
}

/// Backs the hub's data up into `target`, narrating over `on_event`.
///
/// The dump needs the database up; the core starts it for the dump and stops it again
/// if it was down, so the started set is left alone — nothing the user did not start
/// stays running afterwards.
#[command]
pub async fn backup_deployment(
    app: tauri::AppHandle,
    path: String,
    target: String,
    on_event: Channel<backup::BackupEvent>,
) -> Result<backup::BackupReport, String> {
    let request = backup::BackupRequest {
        dir: PathBuf::from(path),
        target: PathBuf::from(target),
    };
    let result = backup::run(&request, &move |event| {
        let _ = on_event.send(event);
    })
    .await
    .map_err(|e| e.to_string());
    crate::tray::poke(&app);
    result
}

// --- installing and starting an engine ----------------------------------------------

use konstruktor_core::remedy::{InstallerId, StartTarget};
use tokio_util::sync::CancellationToken;

/// The installer that is running, if one is, so a Cancel button has something to pull.
///
/// One at a time: two `brew install`s racing for the same lock would only report a lock
/// error, and there is nothing sensible to show for that.
#[derive(Default)]
pub struct InstallState(Mutex<Option<CancellationToken>>);

impl InstallState {
    fn begin(&self) -> Result<CancellationToken, String> {
        let mut slot = self.0.lock().map_err(|e| e.to_string())?;
        if slot.is_some() {
            return Err("an install is already running".into());
        }
        let token = CancellationToken::new();
        *slot = Some(token.clone());
        Ok(token)
    }

    fn end(&self) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = None;
        }
    }

    fn cancel(&self) {
        if let Ok(slot) = self.0.lock() {
            if let Some(token) = slot.as_ref() {
                token.cancel();
            }
        }
    }
}

/// Streamed to the setup panel. Both types are `konstruktor_core::remedy`'s — the
/// executor moved there so `konstruktor doctor --fix` runs the identical plan.
pub use konstruktor_core::remedy::{InstallLine, InstallOutcome};

/// Runs one of the fixed installers, streaming its output over `on_line`.
#[command]
pub async fn install_engine(
    state: tauri::State<'_, InstallState>,
    installer: InstallerId,
    on_line: Channel<InstallLine>,
) -> Result<InstallOutcome, String> {
    let token = state.begin()?;
    let outcome = konstruktor_core::remedy::install(installer, &token, &move |line| {
        let _ = on_line.send(line);
    })
    .await;
    state.end();
    outcome
}

#[command]
pub async fn cancel_install(state: tauri::State<'_, InstallState>) -> Result<(), String> {
    state.cancel();
    Ok(())
}

/// Launches the product behind a stopped daemon, and returns without waiting for it.
#[command]
pub async fn start_engine(target: StartTarget) -> Result<(), String> {
    konstruktor_core::remedy::launch(target).await
}

// --- restore -----------------------------------------------------------------------

#[command]
pub fn read_backup_manifest(backup: String) -> Result<backup::BackupManifest, String> {
    restore::read_manifest(&PathBuf::from(backup)).map_err(|e| e.to_string())
}

/// What restoring `backup` into the hub at `path` would mean — the comparison the dialog
/// shows before asking for the hub's name. Never touches anything.
#[command]
pub async fn restore_plan(
    path: String,
    backup: String,
    method: restore::DbMethod,
    restore_postgres: bool,
    restore_minio: bool,
) -> Result<restore::RestorePlan, String> {
    restore::plan(&restore::RestoreRequest {
        dir: PathBuf::from(path),
        backup: PathBuf::from(backup),
        method,
        restore_postgres,
        restore_minio,
    })
    .await
    .map_err(|e| e.to_string())
}

/// The restore itself. Leaves the stack running — the health check needs it up — so
/// it is counted as started by this app, the way pressing Start would be.
#[command]
pub async fn restore_deployment(
    app: tauri::AppHandle,
    started: tauri::State<'_, StartedStacks>,
    path: String,
    backup: String,
    method: restore::DbMethod,
    restore_postgres: bool,
    restore_minio: bool,
    on_event: Channel<restore::RestoreEvent>,
) -> Result<restore::RestoreReport, String> {
    let request = restore::RestoreRequest {
        dir: PathBuf::from(&path),
        backup: PathBuf::from(backup),
        method,
        restore_postgres,
        restore_minio,
    };
    let result = restore::run(&request, &move |event| {
        let _ = on_event.send(event);
    })
    .await
    .map_err(|e| e.to_string());
    if result.is_ok() {
        started.started(&path);
    }
    crate::tray::poke(&app);
    result
}

#[cfg(test)]
mod wire_shape {
    use super::*;
    use konstruktor_core::config::hub::{build_hub_config, HubConfigOptions};
    use konstruktor_core::status::{ChannelView, ServiceView};

    /// The keys `src/api/types.ts` declares for each of these types.
    ///
    /// Nothing typechecks the frontend against Rust: a renamed or dropped field here
    /// compiles, ships, and shows up as an empty dashboard or a clipboard that copies
    /// `undefined`. These lists are the contract, and they are what makes it safe to move
    /// this module's derivations into `konstruktor-core` — the shape may not move with
    /// them.
    const HUB_STATUS: &[&str] = &[
        "profile",
        "authorized",
        "identifier",
        "authorized_at",
        "gateway_url",
        "admin_user",
        "admin_password",
        "services",
        "mesh_hostname",
        "advertised_port",
        "advertised_hosts",
        "channel",
        "storage",
    ];
    const SERVICE_VIEW: &[&str] = &["id", "name", "host", "url", "image", "tag"];
    const CHANNEL_VIEW: &[&str] = &["tag", "tags"];

    fn keys(value: &serde_json::Value) -> Vec<String> {
        let mut found: Vec<String> = value
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect();
        found.sort();
        found
    }

    fn sorted(names: &[&str]) -> Vec<String> {
        let mut names: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        names.sort();
        names
    }

    fn a_status() -> HubStatus {
        let config = build_hub_config(&HubConfigOptions {
            device_id: "device".into(),
            coord_server: "go.arkitekt.live".into(),
            ..Default::default()
        });
        HubStatus {
            authorized: true,
            identifier: Some("lab-hub".into()),
            authorized_at: Some("2026-01-01T00:00:00Z".into()),
            gateway_url: "http://localhost:7080".into(),
            admin_user: config.global_admin.clone(),
            admin_password: config.global_admin_password.clone(),
            services: vec![ServiceView {
                id: "rekuest".into(),
                name: "Rekuest".into(),
                host: "rekuest".into(),
                url: "http://localhost:7080/rekuest".into(),
                image: Some("jhnnsrs/rekuest:next".into()),
                tag: Some("next".into()),
            }],
            channel: ChannelView {
                tag: Some("next".into()),
                tags: vec!["next".into()],
            },
            storage: konstruktor_core::config::hub::storage_mode_of(&config),
            mesh_hostname: None,
            advertised_port: 7080,
            advertised_hosts: Vec::new(),
            profile: profile::hub_profile(config),
        }
    }

    /// The three types Phase 4 moved into `konstruktor-core`. Each carries
    /// `rename_all = "camelCase"`, and each is read by a component that would silently
    /// render nothing if a key came back snake_case: `EngineSetupPanel` reads
    /// `needsReboot` and `stage`, `BugReportDialog` reads `issueUrl` and `logError`.
    #[test]
    fn the_moved_types_keep_their_camel_case_keys() {
        use konstruktor_core::remedy::{InstallLine, InstallOutcome};
        use konstruktor_core::report::BugReport;

        let line = serde_json::to_value(InstallLine {
            line: "brew install colima".into(),
            stderr: false,
            stage: true,
        })
        .expect("it serializes");
        assert_eq!(keys(&line), sorted(&["line", "stderr", "stage"]));

        let outcome = serde_json::to_value(InstallOutcome {
            ok: false,
            needs_reboot: true,
            cancelled: false,
            message: Some("winget wants a restart".into()),
        })
        .expect("it serializes");
        assert_eq!(
            keys(&outcome),
            sorted(&["ok", "needsReboot", "cancelled", "message"])
        );

        let report = serde_json::to_value(BugReport {
            service: "mikro".into(),
            repo: Some("https://github.com/arkitektio/mikro-server-next".into()),
            issue_url: Some("https://example.invalid/issues/new".into()),
            title: "mikro: ".into(),
            body: "### What happened".into(),
            redactions: 3,
            log_error: None,
        })
        .expect("it serializes");
        assert_eq!(
            keys(&report),
            sorted(&[
                "service",
                "repo",
                "issueUrl",
                "title",
                "body",
                "redactions",
                "logError",
            ])
        );
    }

    #[test]
    fn hub_status_serializes_the_keys_the_frontend_reads() {
        let value = serde_json::to_value(a_status()).expect("it serializes");
        assert_eq!(keys(&value), sorted(HUB_STATUS));
        assert_eq!(keys(&value["services"][0]), sorted(SERVICE_VIEW));
        assert_eq!(keys(&value["channel"]), sorted(CHANNEL_VIEW));
    }

    /// `profile` is passed through whole and the dashboard reads into it, so the envelope
    /// around the config has to survive too.
    #[test]
    fn the_profile_envelope_is_passed_through_whole() {
        let value = serde_json::to_value(a_status()).expect("it serializes");
        assert_eq!(
            keys(&value["profile"]),
            sorted(&["version", "kind", "backend", "config"])
        );
    }
}
