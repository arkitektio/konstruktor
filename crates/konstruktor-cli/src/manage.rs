use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use konstruktor_core::{compose, create, docker, git, profile, registry, status};
use tokio_util::sync::CancellationToken;

use crate::ui;

/// A deployment to act on: a path, a registered name, or — when neither is given — the
/// current directory, if it holds a hub.
#[derive(Args, Debug, Clone)]
pub struct Target {
    /// A path, or the name of a registered deployment.
    pub target: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct CheckoutArgs {
    /// The branch to switch to. Left out, the branches on offer are listed instead.
    pub branch: Option<String>,
    /// The deployment: a path, or the name of a registered one. Defaults to here.
    // A flag rather than the second positional every other command uses. Two optional
    // positionals of different kinds cannot be told apart — `konstruktor checkout .`
    // would be a request for a branch named `.` — and the branch is what this command is
    // for, so the branch is what gets the positional.
    #[arg(long = "in", value_name = "DEPLOYMENT")]
    pub in_deployment: Option<String>,
    /// Only this service. By default every checkout in the deployment is switched, which
    /// is what a dev hub following one branch across the services wants.
    #[arg(long)]
    pub service: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct DownArgs {
    #[command(flatten)]
    pub target: Target,
    /// Also remove the volumes — the database and everything stored in this deployment.
    #[arg(long)]
    pub volumes: bool,
    /// Skip the confirmation. Required when this is not a terminal.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Args, Debug, Clone)]
pub struct SuperuserArgs {
    /// The service whose admin site the account is for. Each keeps its own database, so
    /// an account made in one is not an account in another.
    pub service: String,
    /// The deployment: a path, or the name of a registered one. Defaults to here.
    // A flag, for the reason `CheckoutArgs` gives: two optional positionals of different
    // kinds cannot be told apart, and here the service is the one worth the positional.
    #[arg(long = "in", value_name = "DEPLOYMENT")]
    pub in_deployment: Option<String>,
    #[arg(long)]
    pub username: Option<String>,
    /// Left out, it is asked for without echoing — which is the only way it does not end
    /// up in the shell's history.
    #[arg(long)]
    pub password: Option<String>,
    #[arg(long)]
    pub email: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct BackupArgs {
    /// The folder to back up into. A timestamped subfolder is created inside it.
    pub into: PathBuf,
    /// The deployment: a path, or the name of a registered one. Defaults to here.
    #[arg(long = "in", value_name = "DEPLOYMENT")]
    pub in_deployment: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct RestoreArgs {
    /// The backup folder — the one holding `manifest.json`.
    pub backup: PathBuf,
    /// The deployment to restore into: a path, or the name of a registered one. Defaults
    /// to here.
    #[arg(long = "in", value_name = "DEPLOYMENT")]
    pub in_deployment: Option<String>,
    /// Copy the raw database files back instead of replaying the SQL dump. Only into the
    /// same Postgres major.
    #[arg(long)]
    pub raw: bool,
    /// Leave the database alone.
    #[arg(long)]
    pub skip_postgres: bool,
    /// Leave the object storage alone.
    #[arg(long)]
    pub skip_minio: bool,
    /// Skip the confirmation. Required when this is not a terminal.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Args, Debug, Clone)]
pub struct LogsArgs {
    #[command(flatten)]
    pub target: Target,
    /// Only this service.
    #[arg(long)]
    pub service: Option<String>,
    /// How many existing lines to show first. Defaults to 200, or 20 with --follow,
    /// where the point is what happens next rather than what already happened.
    #[arg(long)]
    pub tail: Option<u32>,
    /// Stay attached and print new lines as they arrive. Ctrl-C stops it.
    #[arg(long, short = 'f')]
    pub follow: bool,
}

/// A deployment that was found: where it is, what it is, and its registry entry if it
/// has one.
///
/// The kind matters because a plugin engine is a real deployment with no hub profile.
/// Resolving used to test for the profile alone, so every engine came back as "there is
/// no hub there any more — it was moved or deleted" while its folder sat untouched.
pub struct Resolved {
    pub dir: PathBuf,
    pub kind: profile::DeploymentKind,
    /// Absent when the target was a bare path that was never registered.
    pub record: Option<registry::DeploymentRecord>,
}

impl Target {
    /// A hub or an engine — for the commands that drive containers, which both have.
    pub fn resolve_any(&self) -> Result<Resolved> {
        let store = registry::load();

        let found = |dir: PathBuf, record: Option<&registry::DeploymentRecord>| {
            let kind = profile::holds_a_deployment(&dir).expect("checked by the caller");
            Resolved {
                dir,
                kind,
                record: record.cloned(),
            }
        };

        match &self.target {
            Some(given) => {
                let as_path = PathBuf::from(given);
                if profile::holds_a_deployment(&as_path).is_some() {
                    let record = registry::find_by_path(&store, &as_path.to_string_lossy());
                    return Ok(found(as_path, record));
                }
                if let Some(record) = registry::find_by_name(&store, given) {
                    let path = PathBuf::from(&record.path);
                    if profile::holds_a_deployment(&path).is_some() {
                        return Ok(found(path, Some(record)));
                    }
                    // A registered deployment whose folder has since been deleted or
                    // moved. Saying so beats a bare "no such file", which reads as a bug.
                    bail!(
                        "`{given}` is registered at {}, but there is nothing there any more — it was moved or deleted.",
                        record.path
                    );
                }
                if as_path.exists() {
                    bail!("{given} does not hold a deployment")
                }
                bail!("no deployment called `{given}`, and no folder at that path")
            }
            None => {
                let here = std::env::current_dir().context("reading the current directory")?;
                if profile::holds_a_deployment(&here).is_some() {
                    let record = registry::find_by_path(&store, &here.to_string_lossy());
                    return Ok(found(here, record));
                }
                bail!(
                    "this directory holds no deployment. Name one — `konstruktor list` \
                     shows what is registered — or give a path."
                )
            }
        }
    }

    /// A hub specifically, for the commands that read its profile.
    ///
    /// An engine is refused by name rather than by a missing file, and the refusal says
    /// what *does* work on one — the alternative is a user concluding their engine is
    /// corrupt.
    pub fn resolve(&self) -> Result<PathBuf> {
        let resolved = self.resolve_any()?;
        match resolved.kind {
            profile::DeploymentKind::Hub => Ok(resolved.dir),
            profile::DeploymentKind::Engine => bail!(
                "{} is a plugin engine, which has no hub profile. `up`, `stop`, `down`, \
                 `pull`, `ps`, `logs`, `restart` and `status` work on it; this command \
                 does not.",
                resolved.dir.display()
            ),
        }
    }
}

pub async fn doctor(json: bool, fix: bool, yes: bool) -> Result<()> {
    let probe = docker::probe().await;
    let git = git::probe();

    if json {
        // Both probes whole, rather than the table's rendering of them: a script wants
        // the verdict and the versions, not the words this prints.
        return ui::emit_json(&serde_json::json!({
            "engine": probe,
            "git": git,
            "ready": probe.is_ready(),
        }));
    }

    ui::say("");
    let rows = vec![
        (
            // The engine names itself: on a Podman machine a row labelled "docker"
            // reporting Podman's version is just confusing.
            probe
                .engine
                .map(|engine| engine.binary_name())
                .unwrap_or("docker")
                .to_string(),
            probe
                .cli_version
                .clone()
                .unwrap_or_else(|| "not found".into()),
        ),
        (
            "compose".to_string(),
            probe
                .compose_version
                .clone()
                .unwrap_or_else(|| "not found".into()),
        ),
        (
            "daemon".to_string(),
            if probe.daemon {
                probe
                    .api_version
                    .clone()
                    .map(|v| format!("API {v}"))
                    .unwrap_or_else(|| "answering".into())
            } else {
                "not answering".into()
            },
        ),
        // Reported, never required: git is only needed for a dev hub, and a plain
        // deployment runs published images without it. It must not decide the verdict.
        (
            "git".to_string(),
            git.cli_version.clone().unwrap_or_else(|| {
                if git.cli {
                    "installed".into()
                } else {
                    "not found".into()
                }
            }),
        ),
    ];
    ui::table(&rows);
    ui::say("");

    if probe.is_ready() {
        ui::ok(&format!("{} is ready.", probe.engine_label()));
        if !git.is_ready() {
            ui::step(&ui::dim(
                "git is not installed. Hubs do not need it — only a dev hub, which checks \
                 the services' source out and mounts it into the containers, does.",
            ));
        }
        ui::say("");
        return Ok(());
    }

    // The remedies are the core's, and the same data the desktop app's setup panel
    // renders — so what `doctor` offers and what the app offers cannot drift apart.
    show_remedies(&probe);

    if !fix {
        if probe.remedies.iter().any(has_runnable_step) {
            ui::step("Run `konstruktor doctor --fix` to apply the recommended remedy.");
            ui::say("");
        }
        // The three failures have three different remedies, worded once in the core.
        return Err(anyhow!(create::CreateError::Docker(
            create::describe_docker(&probe)
        )));
    }

    apply_remedy(&probe, yes).await
}

/// Prints every remedy as numbered steps: the recommended one, then the alternatives.
fn show_remedies(probe: &docker::DockerProbe) {
    use konstruktor_core::remedy::Step;

    for (index, remedy) in probe.remedies.iter().enumerate() {
        ui::say("");
        let heading = if remedy.primary {
            ui::bold(&remedy.title)
        } else {
            format!("{} {}", ui::dim("or"), ui::bold(&remedy.title))
        };
        ui::say(&format!("  {heading}"));
        if !remedy.body.trim().is_empty() {
            ui::step(&ui::dim(&remedy.body));
        }
        for step in &remedy.steps {
            match step {
                Step::OpenUrl { label, url } => {
                    ui::step(&format!("{label}: {}", ui::dim(url)))
                }
                Step::CopyCommand { label, command } => {
                    ui::step(&format!("{label}:"));
                    ui::step(&format!("    {}", ui::bold(command)));
                }
                Step::RunInstaller { label, installer } => {
                    ui::step(&format!("{label}:"));
                    ui::step(&format!("    {}", ui::bold(installer.command())));
                }
                Step::StartEngine { label, .. } => ui::step(label),
                Step::Note { text } => ui::step(&ui::dim(text)),
            }
        }
        let _ = index;
    }
    ui::say("");
}

/// Whether this remedy has a step the CLI could carry out itself, as opposed to one the
/// user has to paste — a Linux install needs `sudo`, and that stays theirs.
fn has_runnable_step(remedy: &konstruktor_core::remedy::Remedy) -> bool {
    use konstruktor_core::remedy::Step;
    remedy
        .steps
        .iter()
        .any(|s| matches!(s, Step::RunInstaller { .. } | Step::StartEngine { .. }))
}

/// `doctor --fix`: run the recommended remedy's runnable steps.
async fn apply_remedy(probe: &docker::DockerProbe, yes: bool) -> Result<()> {
    use konstruktor_core::remedy::{self, Step};

    let Some(remedy) = probe.remedies.iter().find(|r| has_runnable_step(r)) else {
        bail!(
            "nothing here can be fixed automatically — the steps above need a terminal \
             and, on Linux, your own sudo"
        );
    };

    if !yes {
        if !ui::is_interactive() {
            bail!("`--fix` installs software. Pass --yes to confirm.");
        }
        let confirmed = inquire::Confirm::new(&format!("Apply “{}”?", remedy.title))
            .with_default(false)
            .prompt()
            .unwrap_or(false);
        if !confirmed {
            ui::say("");
            ui::step("Left alone.");
            ui::say("");
            return Ok(());
        }
    }

    let cancel = CancellationToken::new();
    let on_signal = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            on_signal.cancel();
        }
    });

    ui::say("");
    for step in &remedy.steps {
        match step {
            Step::RunInstaller { installer, .. } => {
                // The same executor the desktop app runs, streaming the same lines.
                let outcome = remedy::install(*installer, &cancel, &|line| {
                    if line.stage {
                        ui::say("");
                        ui::step(&ui::bold(&line.line));
                    } else {
                        ui::say(&format!("    {}", ui::dim(&line.line)));
                    }
                })
                .await
                .map_err(|e| anyhow!("{e}"))?;

                if outcome.cancelled {
                    ui::say("");
                    ui::step("Cancelled.");
                    ui::say("");
                    return Ok(());
                }
                if !outcome.ok {
                    bail!(
                        "{}",
                        outcome.message.unwrap_or_else(|| "the installer failed".into())
                    );
                }
                if outcome.needs_reboot {
                    ui::say("");
                    ui::warn("Windows has to restart before the engine can start.");
                }
            }
            Step::StartEngine { target, .. } => {
                ui::step(&format!("Starting {}…", target.label()));
                remedy::launch(*target).await.map_err(|e| anyhow!("{e}"))?;
            }
            // Everything else is for a human: a page to read, or a command needing sudo.
            _ => {}
        }
    }

    ui::say("");
    ui::progress("Checking again…");
    let after = docker::probe().await;
    ui::end_progress();
    if after.is_ready() {
        ui::ok(&format!("{} is ready.", after.engine_label()));
        ui::say("");
        Ok(())
    } else {
        ui::say("");
        bail!(
            "still not ready — {}. It may need a moment, or a new terminal.",
            create::describe_docker(&after)
        )
    }
}

/// The dev hub's checkouts: what branch each is on, and how to move them.
///
/// One command rather than a `branch` and a `checkout`: without a branch it lists, with
/// one it switches. The switch refuses over uncommitted work, and says so per service
/// rather than stopping at the first — a partial answer here is worse than a full report.
pub fn checkout(args: &CheckoutArgs) -> Result<()> {
    let dir = Target {
        target: args.in_deployment.clone(),
    }
    .resolve()?;
    let profile = profile::read_profile(&dir)?;
    let checkouts = git::checkouts(&dir, &profile.config);

    if checkouts.is_empty() {
        bail!(
            "this deployment runs published images, so there is nothing to check out. \
             A dev hub is created with `konstruktor hub create --dev`."
        )
    }

    // The mistake the flag above exists to prevent, caught rather than acted on: without
    // this, `konstruktor checkout .` would try to put every service on a branch named `.`
    // and report five identical failures instead of the one useful sentence.
    if let Some(given) = args.branch.as_deref().map(str::trim) {
        if given == "." || given == ".." || given.contains('/') && Path::new(given).exists() {
            bail!("`{given}` looks like a folder. Name the deployment with `--in {given}`.")
        }
    }

    let Some(branch) = args
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
    else {
        ui::say("");
        for checkout in &checkouts {
            let state = match (&checkout.error, &checkout.branch) {
                (Some(error), _) => error.clone(),
                (None, Some(branch)) => {
                    format!(
                        "{branch}{}",
                        if checkout.dirty {
                            " (uncommitted changes)"
                        } else {
                            ""
                        }
                    )
                }
                (None, None) => "detached HEAD".to_string(),
            };
            ui::say(&format!("  {}", ui::bold(&checkout.service)));
            ui::say(&format!("    {}", ui::dim(&state)));

            // Nothing to offer for a checkout that could not be read, and asking git
            // anyway only prints a second, worse phrasing of the same problem.
            if checkout.error.is_none() {
                match git::branches(Path::new(&checkout.path)) {
                    Ok(names) => ui::say(&format!("    {}", ui::dim(&names.join("  ")))),
                    Err(error) => ui::say(&format!("    {}", ui::dim(&error))),
                }
            }
        }
        ui::say("");
        return Ok(());
    };

    let wanted: Vec<_> = match &args.service {
        Some(name) => {
            let found: Vec<_> = checkouts.iter().filter(|c| &c.service == name).collect();
            if found.is_empty() {
                bail!("this deployment has no checkout for `{name}`")
            }
            found
        }
        None => checkouts.iter().collect(),
    };

    ui::say("");
    let mut moved: Vec<&str> = Vec::new();
    let mut failed: Vec<&str> = Vec::new();
    for checkout in wanted {
        match git::switch_branch(&checkout.service, Path::new(&checkout.path), branch) {
            Ok(()) => {
                moved.push(&checkout.service);
                ui::ok(&format!("{} is on {branch}", checkout.service));
            }
            Err(error) => {
                failed.push(&checkout.service);
                ui::warn(&format!("{}: {error}", checkout.service));
            }
        }
    }
    ui::say("");

    if !failed.is_empty() {
        // Saying only what failed would understate it: the rest of the hub *did* move,
        // so the deployment is now split across two branches. That is a state worth
        // spelling out, because the way out of it is to name the services explicitly.
        if moved.is_empty() {
            bail!("nothing moved; {} was left where it was", failed.join(", "))
        }
        bail!(
            "this hub is now split: {} moved to {branch}, {} did not. Fix the ones that \
             refused, or put the others back with `--service`.",
            moved.join(", "),
            failed.join(", "),
        )
    }
    // The containers hold whatever they loaded at start; the code on disk is not what is
    // running until they are recreated.
    ui::step(&ui::dim(
        "Recreate the stack with `konstruktor up` for the containers to run this branch.",
    ));
    ui::say("");
    Ok(())
}

pub fn list(json: bool) -> Result<()> {
    let store = registry::load();

    if json {
        return ui::emit_json(&store.deployments);
    }

    if store.deployments.is_empty() {
        ui::say("");
        ui::step("No deployments yet. Create one with `konstruktor hub create`.");
        ui::say("");
        return Ok(());
    }

    ui::say("");
    for record in &store.deployments {
        ui::say(&format!("  {}", ui::bold(&record.name)));
        ui::say(&format!("    {}", ui::dim(&record.path)));
        if let Some(server) = &record.coord_server {
            let identifier = record.identifier.as_deref().unwrap_or("—");
            ui::say(&format!(
                "    {}",
                ui::dim(&format!("{identifier} at {server}"))
            ));
        }
    }
    ui::say("");
    Ok(())
}

/// What a plugin engine can say about itself: the registry knows its coordination
/// server, and the daemon knows whether its container is up. There is no profile.
async fn engine_status(resolved: &Resolved) -> Result<()> {
    ui::say("");
    let mut rows = vec![
        ("folder".into(), resolved.dir.to_string_lossy().to_string()),
        ("kind".into(), "plugin engine".to_string()),
    ];
    if let Some(record) = &resolved.record {
        rows.push(("name".into(), record.name.clone()));
        if let Some(server) = &record.coord_server {
            rows.push(("coordination".into(), server.clone()));
        }
        if let Some(identifier) = &record.identifier {
            rows.push(("identifier".into(), identifier.clone()));
        }
    }
    ui::table(&rows);
    report_containers(&resolved.dir).await;
    ui::say("");
    Ok(())
}

/// The container readout both kinds of status end with.
async fn report_containers(dir: &Path) {
    // A nice-to-have: a stopped daemon must not fail `status`.
    match docker::list_deployment_containers(&dir.to_string_lossy()).await {
        Ok(containers) if !containers.is_empty() => {
            let summary = status::run_summary(&containers);
            ui::say("");
            for container in &containers {
                let name = container.service.clone().unwrap_or_else(|| "?".into());
                let state = container.state.clone().unwrap_or_else(|| "unknown".into());
                ui::say(&format!("  {:16}  {}", name, ui::dim(&state)));
            }
            ui::say("");
            ui::step(&format!(
                "{} — {}/{} running.",
                summary.state.label(),
                summary.running,
                summary.total
            ));
        }
        Ok(_) => {
            ui::say("");
            ui::step(&ui::dim("Nothing running."));
        }
        Err(e) => {
            ui::say("");
            ui::step(&ui::dim(&format!("Could not reach docker: {e}")));
        }
    }
}

pub async fn status(target: &Target, json: bool) -> Result<()> {
    let resolved = target.resolve_any()?;
    let dir = resolved.dir.clone();

    if json {
        let containers = docker::list_deployment_containers(&dir.to_string_lossy())
            .await
            .unwrap_or_default();
        // The hub view is the dashboard's own model; an engine has none, and says so
        // with a null rather than by failing.
        let hub = match resolved.kind {
            profile::DeploymentKind::Hub => Some(status::hub_view(&dir)?),
            profile::DeploymentKind::Engine => None,
        };
        return ui::emit_json(&serde_json::json!({
            "path": dir,
            "kind": resolved.kind,
            "record": resolved.record,
            "hub": hub,
            "run": status::run_summary(&containers),
            "containers": containers,
        }));
    }

    // An engine has no hub profile to describe — one deployer container is the whole of
    // it — so it gets the part of this that is true for both: where it is and what is up.
    if resolved.kind == profile::DeploymentKind::Engine {
        return engine_status(&resolved).await;
    }

    // The same view the dashboard renders, derived in the core — so the two front ends
    // cannot disagree about a hub's gateway address or its release channel.
    let view = status::hub_view(&dir)?;
    let config = &view.profile.config;

    ui::say("");
    let mut rows = vec![
        ("folder".into(), dir.to_string_lossy().to_string()),
        ("gateway".into(), view.gateway_url.clone()),
        ("coordination".into(), config.coord_server.clone()),
        (
            "rekuest".into(),
            if config.rekuest_server.trim() == "local" {
                "runs here".to_string()
            } else {
                config.rekuest_server.clone()
            },
        ),
        (
            "services".into(),
            view.services
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ),
        (
            "channel".into(),
            match (&view.channel.tag, view.channel.tags.len()) {
                (Some(tag), _) => tag.clone(),
                (None, 0) => "not pinned".into(),
                // Worth spelling out rather than blanking: a mixed hub is usually a
                // half-finished update, and the tags are the evidence.
                (None, _) => format!("mixed — {}", view.channel.tags.join(", ")),
            },
        ),
        (
            "storage".into(),
            match view.storage {
                konstruktor_core::config::hub::StorageMode::DockerVolumes => {
                    "docker volumes".into()
                }
                konstruktor_core::config::hub::StorageMode::DeploymentFolder => {
                    "deployment folder".to_string()
                }
            },
        ),
    ];

    match &view.mesh_hostname {
        Some(hostname) => rows.push(("mesh".into(), format!("joins as {hostname}"))),
        None => rows.push(("mesh".into(), "not joined".into())),
    }

    match (&view.identifier, &view.authorized_at) {
        (Some(identifier), Some(at)) => {
            rows.push(("authorized".into(), format!("{at} as {identifier}")))
        }
        _ => rows.push(("authorized".into(), "not yet".into())),
    }

    ui::table(&rows);
    report_containers(&dir).await;

    ui::say("");
    Ok(())
}

/// Runs a compose subcommand in the deployment folder, streaming its output through.
pub fn compose(target: &Target, args: Vec<&str>, verb: &str) -> Result<()> {
    let dir = target.resolve_any()?.dir;
    ui::say("");
    ui::step(&format!("{verb} {}…", ui::bold(&dir.to_string_lossy())));

    let status = konstruktor_core::docker::command()
        .args(&args)
        .current_dir(&dir)
        .status()
        .context("running docker")?;

    ui::say("");
    if status.success() {
        ui::ok("Done.");
        ui::say("");
        Ok(())
    } else {
        bail!("docker {} exited with {}", args.join(" "), status)
    }
}

pub fn down(args: DownArgs) -> Result<()> {
    if args.volumes && !args.yes {
        // The only destructive path there is, so it is the only one that asks.
        if !ui::is_interactive() {
            bail!(
                "`down --volumes` deletes the database and everything stored in this \
                 deployment. Pass --yes to confirm."
            );
        }
        let dir = args.target.resolve_any()?.dir;
        ui::say("");
        ui::warn(&format!(
            "This deletes the database and object storage in {}. It cannot be undone.",
            dir.to_string_lossy()
        ));
        let confirmed = inquire::Confirm::new("Delete the data?")
            .with_default(false)
            .prompt()
            .unwrap_or(false);
        if !confirmed {
            ui::say("");
            ui::step("Left alone.");
            ui::say("");
            return Ok(());
        }
    }

    let argv = if args.volumes {
        compose::down_volumes()
    } else {
        compose::down()
    };
    self::compose(&args.target, argv, "Removing containers for")
}

pub async fn ps(target: &Target, json: bool) -> Result<()> {
    let dir = target.resolve_any()?.dir;

    if json {
        let containers = docker::list_deployment_containers(&dir.to_string_lossy())
            .await
            .map_err(|e| anyhow!("{e}"))?;
        return ui::emit_json(&serde_json::json!({
            "containers": containers,
            "run": status::run_summary(&containers),
        }));
    }

    let status = konstruktor_core::docker::command()
        .args(compose::ps())
        .current_dir(&dir)
        .status()
        .context("running docker")?;
    if !status.success() {
        bail!("docker compose ps exited with {status}");
    }
    Ok(())
}

/// `konstruktor superuser <service>`: an admin account in one running service.
///
/// The same `docker compose exec` the desktop app runs. Deliberately not part of
/// creating a hub — the container has to be up and its migrations applied before there
/// is a table to write to.
pub fn superuser(args: SuperuserArgs) -> Result<()> {
    let dir = Target {
        target: args.in_deployment.clone(),
    }
    .resolve()?;

    let username = match args.username {
        Some(name) => name,
        None if ui::is_interactive() => inquire::Text::new("Username")
            .with_default("admin")
            .prompt()
            .context("reading the username")?,
        None => bail!("--username is required when this is not a terminal"),
    };

    let password = match args.password {
        Some(password) => password,
        None if ui::is_interactive() => inquire::Password::new("Password")
            .with_display_mode(inquire::PasswordDisplayMode::Masked)
            .prompt()
            .context("reading the password")?,
        None => bail!("--password is required when this is not a terminal"),
    };

    if username.trim().is_empty() || password.is_empty() {
        bail!("a username and a password are both required");
    }

    let argv = compose::create_superuser(
        &args.service,
        username.trim(),
        &password,
        args.email
            .as_deref()
            .map(str::trim)
            .filter(|e| !e.is_empty()),
    );

    ui::say("");
    ui::step(&format!(
        "Creating {} in {}…",
        ui::bold(username.trim()),
        ui::bold(&args.service)
    ));

    let output = konstruktor_core::docker::command()
        .args(&argv)
        .current_dir(&dir)
        .output()
        .context("running docker")?;

    if !output.status.success() {
        // Django's own complaint — "that username is already taken" and the like — is
        // what the user needs, not the exit code.
        let message = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        bail!("{}", message.trim());
    }

    ui::say("");
    ui::ok(&format!(
        "{} can now sign in to {}'s admin site.",
        username.trim(),
        args.service
    ));
    ui::say("");
    Ok(())
}

pub fn logs(args: LogsArgs) -> Result<()> {
    let dir = args.target.resolve_any()?.dir;
    let tail = args.tail.unwrap_or(if args.follow { 20 } else { 200 });
    let argv = compose::logs_following(args.service.as_deref(), tail, args.follow);

    let status = konstruktor_core::docker::command()
        .args(&argv)
        .current_dir(&dir)
        .status()
        .context("running docker")?;

    // Ctrl-C is how a follow is *meant* to end. The terminal sends SIGINT to the whole
    // foreground group, so compose exits 130 — reporting that as a failure would make
    // every successful `logs -f` look like one.
    if args.follow && was_interrupted(&status) {
        ui::say("");
        return Ok(());
    }
    if !status.success() {
        bail!("docker {} exited with {status}", argv.join(" "));
    }
    Ok(())
}

/// Whether a child died because the terminal sent SIGINT to the foreground group.
///
/// The signal, and only the signal. A process killed by one has no exit code at all, so
/// there is nothing to be gained by also accepting 130 — and something to lose: a command
/// that genuinely exits 130 for its own reasons would be reported as a clean stop.
#[cfg(unix)]
fn was_interrupted(status: &std::process::ExitStatus) -> bool {
    use std::os::unix::process::ExitStatusExt;
    status.signal() == Some(2)
}

#[cfg(not(unix))]
fn was_interrupted(status: &std::process::ExitStatus) -> bool {
    // Windows has no signal to read; Ctrl-C surfaces as this status.
    status.code() == Some(130) || status.code() == Some(0xC000_013A_u32 as i32)
}

/// `konstruktor backup <folder>`: a `pg_dumpall`, a copy of the database files, a copy of
/// the object storage, and the deployment's own configuration, in a timestamped folder.
///
/// The copies run in a container with `rsync` — the data is in a Docker volume by
/// default, which nothing on the host can read directly. See `konstruktor_core::backup`.
pub async fn backup(args: BackupArgs) -> Result<()> {
    use konstruktor_core::backup::{self, BackupEvent, BackupRequest};

    let dir = Target {
        target: args.in_deployment.clone(),
    }
    .resolve()?;
    let request = BackupRequest {
        dir,
        target: args.into.clone(),
    };

    ui::say("");
    ui::step(&format!(
        "Backing {} up into {}…",
        ui::bold(&request.dir.to_string_lossy()),
        ui::bold(&request.target.to_string_lossy())
    ));

    let report = backup::run(&request, &|event| match event {
        BackupEvent::Step { title, .. } => {
            ui::say("");
            ui::step(&title);
        }
        BackupEvent::Line { line, .. } => ui::say(&ui::dim(&format!("  {line}"))),
        BackupEvent::Skipped { reason, .. } => ui::warn(&format!("skipped — {reason}")),
    })
    .await?;

    ui::say("");
    ui::ok(&format!("Backup written to {}", report.path));
    for warning in &report.warnings {
        ui::warn(warning);
    }
    ui::say("");
    // The one line a script wants.
    println!("{}", report.path);
    Ok(())
}

/// `konstruktor restore <backup>`: the plan first — what the backup holds against what
/// this hub runs — then, once confirmed, the restore and the health check.
pub async fn restore(args: RestoreArgs) -> Result<()> {
    use konstruktor_core::restore::{self, DbMethod, RestoreEvent, RestoreRequest, Verdict};

    let dir = Target {
        target: args.in_deployment.clone(),
    }
    .resolve()?;
    let request = RestoreRequest {
        dir,
        backup: args.backup.clone(),
        method: if args.raw { DbMethod::Raw } else { DbMethod::Dump },
        restore_postgres: !args.skip_postgres,
        restore_minio: !args.skip_minio,
    };

    ui::say("");
    ui::step(&format!(
        "Comparing {} with {}…",
        ui::bold(&request.backup.to_string_lossy()),
        ui::bold(&request.dir.to_string_lossy())
    ));
    let plan = restore::plan(&request).await?;

    ui::say("");
    ui::table(&[
        (
            "backup of".into(),
            plan.manifest.hub.identifier.clone().unwrap_or_else(|| "unauthorized hub".into()),
        ),
        (
            "taken".into(),
            konstruktor_core::backup::timestamp(plan.manifest.taken_at),
        ),
        (
            "same hub".into(),
            if plan.same_hub { "yes".into() } else { "no".into() },
        ),
    ]);
    ui::say("");
    let mut rows: Vec<(String, String)> = plan
        .services
        .iter()
        .map(|s| {
            (
                s.host.clone(),
                format!(
                    "{}  →  {}  [{}]",
                    s.backup_image,
                    s.deployed_image.as_deref().unwrap_or("not deployed"),
                    match s.verdict {
                        Verdict::Same => "same",
                        Verdict::DifferentTag => "different tag",
                        Verdict::DifferentBuild => "different build",
                        Verdict::MissingInTarget => "MISSING",
                        Verdict::NotResolvable => "same tag",
                    }
                ),
            )
        })
        .collect();
    for extra in &plan.extra_in_target {
        rows.push((extra.as_str().into(), "runs here, nothing in the backup".into()));
    }
    rows.push((
        "db".into(),
        format!("{}  →  {}", plan.db.backup_image, plan.db.deployed_image),
    ));
    ui::table(&rows);

    ui::say("");
    for warning in &plan.warnings {
        ui::warn(warning);
    }
    if !plan.blocking.is_empty() {
        for reason in &plan.blocking {
            ui::fail(reason);
        }
        bail!("the restore cannot go ahead");
    }

    if !args.yes {
        if !ui::is_interactive() {
            bail!("this replaces the hub's data; pass -y to confirm without a terminal");
        }
        let ok = inquire::Confirm::new("Replace this hub's data with the backup?")
            .with_default(false)
            .prompt()?;
        if !ok {
            bail!("cancelled");
        }
    }

    let report = restore::run(&request, &|event| match event {
        RestoreEvent::Step { title, .. } => {
            ui::say("");
            ui::step(&title);
        }
        RestoreEvent::Line { line, .. } => ui::say(&ui::dim(&format!("  {line}"))),
        RestoreEvent::Skipped { reason, .. } => ui::warn(&format!("skipped — {reason}")),
        RestoreEvent::Checked { service, healthy, detail } => {
            if healthy {
                ui::ok(&format!("{service}: {detail}"));
            } else {
                ui::fail(&format!("{service}: {detail}"));
            }
        }
    })
    .await?;

    ui::say("");
    for warning in &report.warnings {
        ui::warn(warning);
    }
    if report.all_healthy {
        ui::ok(&format!(
            "Restored, and all {} services answer.",
            report.health.len()
        ));
        ui::say("");
        Ok(())
    } else {
        bail!("restored, but not every service is healthy — see above")
    }
}

// --- removing a deployment -------------------------------------------------------------
//
// Three commands because these are three different amounts of destruction, and conflating
// them is how people lose data they meant to keep. `forget` touches no files at all;
// `purge` removes the data and leaves the hub; `destroy` removes everything.

#[derive(Args, Debug, Clone)]
pub struct DestroyArgs {
    #[command(flatten)]
    pub target: Target,
    /// Skip the confirmation. Required when this is not a terminal.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

/// The registry entry behind a target, which the destructive paths are addressed by.
///
/// `destroy` and `purge` take an id rather than a path on purpose — the id is what proves
/// Konstruktor created this folder, and a bare path would let either be pointed at
/// anything. So an unregistered folder is refused here rather than deep inside the core.
fn record_for(target: &Target) -> Result<(PathBuf, registry::DeploymentRecord)> {
    let resolved = target.resolve_any()?;
    match resolved.record {
        Some(record) => Ok((resolved.dir, record)),
        None => bail!(
            "{} is not a registered deployment, so Konstruktor will not delete it. \
             `konstruktor list` shows what is registered.",
            resolved.dir.display()
        ),
    }
}

/// Prints what an action is about to take, and everything it deliberately will not.
fn show_plan(plan: &konstruktor_core::destroy::DeletionPlan, whole_folder: bool) {
    ui::say("");
    if whole_folder {
        ui::warn(&format!("This deletes {} and everything in it.", plan.path));
    } else {
        ui::warn(&format!("This deletes the data in {}.", plan.path));
    }

    if plan.storage.uses_volumes() {
        ui::step(&ui::dim("The database and object storage are in docker volumes."));
    }
    for dir in &plan.data_dirs {
        ui::step(&ui::dim(&format!("removes {dir}")));
    }
    // Unpushed work is the one loss nothing can undo, so it is said loudest.
    if !plan.checkouts.is_empty() {
        ui::warn(&format!(
            "{} source checkout(s) here may hold commits that exist nowhere else:",
            plan.checkouts.len()
        ));
        for checkout in &plan.checkouts {
            ui::step(&ui::dim(checkout));
        }
    }
    if plan.on_a_mesh {
        ui::warn(
            "This hub is on a mesh. Its tailnet state is a volume, and the key that \
             joined it was single-use — it cannot simply rejoin.",
        );
    }
    if plan.was_authorized {
        ui::step(&ui::dim(
            "It holds an identifier on a coordination server, which this cannot revoke.",
        ));
    }
    for skipped in &plan.skipped {
        ui::step(&ui::dim(&format!(
            "left alone: {} — {}",
            skipped.mount, skipped.explanation
        )));
    }
    ui::say("");
}

/// Asks before something irreversible. Typing the name is the desktop app's gate for the
/// one action that removes files the user wrote, and it is worth the keystrokes here too.
fn confirm_destruction(name: &str, yes: bool, by_name: bool, what: &str) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !ui::is_interactive() {
        bail!("{what} cannot be undone. Pass --yes to confirm.");
    }
    if by_name {
        let typed = inquire::Text::new(&format!("Type `{name}` to confirm"))
            .prompt()
            .unwrap_or_default();
        if typed.trim() != name {
            return Ok(false);
        }
        return Ok(true);
    }
    Ok(inquire::Confirm::new(what)
        .with_default(false)
        .prompt()
        .unwrap_or(false))
}

/// `konstruktor destroy`: the stack, the folder and the registry entry.
pub fn destroy(args: DestroyArgs) -> Result<()> {
    use konstruktor_core::destroy;

    let (_, record) = record_for(&args.target)?;
    let (_, plan) = destroy::plan(&record)?;
    show_plan(&plan, true);

    if !confirm_destruction(&plan.name, args.yes, true, "Deleting this deployment")? {
        ui::step("Left alone.");
        ui::say("");
        return Ok(());
    }

    ui::step(&format!("Deleting {}…", ui::bold(&plan.name)));
    let done = destroy::delete(&record.id)?;
    ui::say("");
    // Per step, because "what is still on my machine" is the question after a failure.
    for (label, ok) in [
        ("containers, networks and volumes", done.stack_removed),
        ("the folder", done.folder_removed),
        ("the registry entry", done.forgotten),
    ] {
        if ok {
            ui::ok(&format!("removed {label}"));
        } else {
            ui::fail(&format!("could not remove {label}"));
        }
    }
    ui::say("");
    Ok(())
}

/// `konstruktor purge`: the data, and nothing else. The hub stays, and can be started
/// again into an empty database.
pub fn purge(args: DestroyArgs) -> Result<()> {
    use konstruktor_core::destroy;

    let (_, record) = record_for(&args.target)?;
    let (_, plan) = destroy::plan(&record)?;
    show_plan(&plan, false);

    if !confirm_destruction(&plan.name, args.yes, false, "Delete the data?")? {
        ui::step("Left alone.");
        ui::say("");
        return Ok(());
    }

    ui::step(&format!("Purging {}…", ui::bold(&plan.name)));
    let purged = destroy::purge_data(&record.id)?;
    ui::say("");
    if purged.stack_removed {
        ui::ok("stack taken down");
    }
    for dir in &purged.removed {
        ui::ok(&format!("removed {dir}"));
    }
    for skipped in &purged.skipped {
        ui::warn(&format!("left {} — {}", skipped.mount, skipped.explanation));
    }
    if purged.removed.is_empty() && !purged.skipped.is_empty() {
        ui::step(&ui::dim("Nothing on the host to remove."));
    }
    ui::say("");
    ui::step("The hub is still here. `konstruktor up` starts it with an empty database.");
    ui::say("");
    Ok(())
}

/// `konstruktor forget`: stop listing it. Nothing on disk is touched.
pub fn forget(target: &Target) -> Result<()> {
    let (dir, record) = record_for(target)?;
    let mut store = registry::load();
    store.deployments.retain(|d| d.id != record.id);
    registry::save(&store).context("writing the registry")?;

    ui::say("");
    ui::ok(&format!("Konstruktor no longer lists {}.", record.name));
    ui::step(&ui::dim(&format!(
        "Everything in {} is untouched.",
        dir.display()
    )));
    ui::say("");
    Ok(())
}

#[derive(Args, Debug, Clone)]
pub struct RestartArgs {
    /// The service to restart. Left out, every container in the deployment is restarted.
    pub service: Option<String>,
    /// The deployment: a path, or the name of a registered one. Defaults to here.
    #[arg(long = "in", value_name = "DEPLOYMENT")]
    pub in_deployment: Option<String>,
}

/// `konstruktor restart [service]`: bounce one container, or all of them.
///
/// A restart rather than a `down`/`up`: the container keeps its identity and its volumes,
/// which is what "it has wedged, kick it" means. Recreating against a newer image is
/// `update`'s job.
pub async fn restart(args: RestartArgs) -> Result<()> {
    let dir = Target {
        target: args.in_deployment.clone(),
    }
    .resolve_any()?
    .dir;

    let containers = docker::list_deployment_containers(&dir.to_string_lossy())
        .await
        .map_err(|e| anyhow!("{e}"))?;

    let wanted: Vec<&konstruktor_core::docker::Container> = containers
        .iter()
        .filter(|c| match &args.service {
            Some(service) => c.service.as_deref() == Some(service.as_str()),
            None => true,
        })
        .collect();

    if wanted.is_empty() {
        match &args.service {
            Some(service) => bail!(
                "no container for `{service}` in this deployment — `konstruktor ps` lists \
                 what is running"
            ),
            None => bail!("nothing is running in this deployment"),
        }
    }

    ui::say("");
    for container in wanted {
        let name = container.service.clone().unwrap_or_else(|| "?".into());
        let Some(id) = container.id.as_deref() else {
            ui::warn(&format!("{name} has no container id — skipped"));
            continue;
        };
        ui::progress(&format!("restarting {name}…"));
        match docker::restart_container(id).await {
            Ok(()) => {
                ui::end_progress();
                ui::ok(&name);
            }
            Err(e) => {
                ui::end_progress();
                ui::fail(&format!("{name}: {e}"));
            }
        }
    }
    ui::say("");
    Ok(())
}

#[derive(Args, Debug, Clone)]
pub struct OpenArgs {
    #[command(flatten)]
    pub target: Target,
    /// Open one service's path through the gateway rather than the gateway root.
    #[arg(long)]
    pub service: Option<String>,
    /// Print the address instead of opening it.
    #[arg(long)]
    pub print: bool,
}

/// `konstruktor open`: the hub in a browser.
///
/// The address comes from the same core derivation the dashboard shows, so this can never
/// point somewhere the app would not.
pub fn open(args: OpenArgs) -> Result<()> {
    let dir = args.target.resolve()?;
    let view = status::hub_view(&dir)?;

    let url = match &args.service {
        Some(service) => {
            let found = view
                .services
                .iter()
                .find(|s| s.id == *service || s.host == *service);
            match found {
                Some(service) => service.url.clone(),
                None => bail!(
                    "this hub runs no service called `{service}` — it runs {}",
                    view.services
                        .iter()
                        .map(|s| s.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        }
        None => view.gateway_url.clone(),
    };

    // The address goes to stdout either way: it is the machine-readable answer, and a
    // browser that fails to open must not take it with it.
    println!("{url}");
    if !args.print {
        ui::open_in_browser(&url);
    }
    Ok(())
}

#[derive(Args, Debug, Clone)]
pub struct UpdateArgs {
    #[command(flatten)]
    pub target: Target,
    /// Only report what has moved upstream; change nothing.
    #[arg(long)]
    pub check: bool,
    /// Only this service. By default every service with something newer is updated.
    #[arg(long)]
    pub service: Option<String>,
    /// Skip the confirmation. Required when this is not a terminal.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

/// `konstruktor update`: pull and recreate only what actually moved.
///
/// `pull` fetches every image whether or not anything changed, and leaves the running
/// containers on the old ones until the next `up`. This asks each registry whether the tag
/// has moved, then recreates just those services — which is what the dashboard's update
/// card has always done and the CLI could not.
pub async fn update(args: UpdateArgs, json: bool) -> Result<()> {
    use konstruktor_core::updates::{self, UpstreamState};

    let dir = args.target.resolve()?;

    if !json {
        ui::say("");
        ui::progress("Asking the registries what has moved…");
    }
    let checks = updates::for_deployment(&dir)
        .await
        .map_err(|e| anyhow!("{e}"))?;
    if !json {
        ui::end_progress();
    }

    if json {
        // Reporting only: applying updates streams docker's output, which has no place
        // in a JSON document.
        if !args.check {
            bail!("--json reports; pass --check with it, or drop --json to apply");
        }
        return ui::emit_json(&checks);
    }

    let wanted: Vec<&updates::UpstreamCheck> = checks
        .iter()
        .filter(|c| match &args.service {
            Some(service) => c.service == *service,
            None => true,
        })
        .collect();

    if let (Some(service), true) = (&args.service, wanted.is_empty()) {
        bail!("this hub runs no service called `{service}`");
    }

    let rows: Vec<(String, String)> = wanted
        .iter()
        .map(|check| {
            let state = match check.state {
                UpstreamState::Current => "up to date".to_string(),
                UpstreamState::Newer => ui::bold("newer available"),
                UpstreamState::Missing => "not pulled yet".to_string(),
                UpstreamState::Unknown => match &check.error {
                    Some(error) => format!("could not check — {error}"),
                    None => "could not check".to_string(),
                },
            };
            (check.service.clone(), state)
        })
        .collect();
    ui::table(&rows);
    ui::say("");

    // Missing counts: nothing has pulled it yet, so there is something to fetch either way.
    let stale: Vec<&updates::UpstreamCheck> = wanted
        .into_iter()
        .filter(|c| matches!(c.state, UpstreamState::Newer | UpstreamState::Missing))
        .collect();

    if stale.is_empty() {
        ui::ok("Everything is up to date.");
        ui::say("");
        return Ok(());
    }

    let names: Vec<&str> = stale.iter().map(|c| c.service.as_str()).collect();
    if args.check {
        ui::step(&format!(
            "{} would be updated: {}",
            stale.len(),
            names.join(", ")
        ));
        ui::step(&ui::dim("Run `konstruktor update` to apply."));
        ui::say("");
        return Ok(());
    }

    if !args.yes {
        if !ui::is_interactive() {
            bail!("this restarts {}. Pass --yes to confirm.", names.join(", "));
        }
        let confirmed = inquire::Confirm::new(&format!("Update {}?", names.join(", ")))
            .with_default(true)
            .prompt()
            .unwrap_or(false);
        if !confirmed {
            ui::say("");
            ui::step("Left alone.");
            ui::say("");
            return Ok(());
        }
    }

    for check in &stale {
        ui::step(&format!("{}…", ui::bold(&check.service)));
        // Pull then recreate, per service. `up_service` carries --no-deps for a reason
        // its doc comment spells out: without it, updating one service on a stopped stack
        // would quietly boot the infrastructure and leave the hub half up.
        for argv in [
            compose::pull_service(&check.service),
            compose::up_service(&check.service),
        ] {
            let status = konstruktor_core::docker::command()
                .args(&argv)
                .current_dir(&dir)
                .status()
                .context("running docker")?;
            if !status.success() {
                bail!("docker {} exited with {status}", argv.join(" "));
            }
        }
    }

    ui::say("");
    ui::ok(&format!("Updated {}.", names.join(", ")));
    ui::say("");
    Ok(())
}

#[derive(Args, Debug, Clone)]
pub struct ReportArgs {
    /// The service the report is about, by its compose name.
    pub service: String,
    /// The deployment: a path, or the name of a registered one. Defaults to here.
    #[arg(long = "in", value_name = "DEPLOYMENT")]
    pub in_deployment: Option<String>,
    /// Open the prefilled issue page in a browser.
    #[arg(long)]
    pub open: bool,
}

/// `konstruktor report <service>`: a bug report for the service that is misbehaving.
///
/// The document goes to stdout so `konstruktor report mikro > issue.md` does the obvious
/// thing; the count of redacted values goes to stderr, because a claim that credentials
/// were removed is one the user is entitled to see and a pipe must not swallow it.
pub async fn report(args: ReportArgs) -> Result<()> {
    let dir = Target {
        target: args.in_deployment.clone(),
    }
    .resolve_any()?
    .dir;

    let report = konstruktor_core::report::bug_report(
        &dir,
        args.service.clone(),
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .map_err(|e| anyhow!("{e}"))?;

    println!("{}", report.body);

    ui::say("");
    if report.redactions > 0 {
        ui::ok(&format!(
            "{} secret value(s) were replaced with `[redacted: …]`.",
            report.redactions
        ));
    } else {
        ui::step(&ui::dim(
            "No values matching this deployment's own configuration were found in the log.",
        ));
    }
    if let Some(error) = &report.log_error {
        ui::warn(&format!("The log could not be read: {error}"));
    }

    match (&report.issue_url, args.open) {
        (Some(url), true) => {
            ui::step(&format!("Opening {}", ui::dim(url)));
            ui::open_in_browser(url);
        }
        (Some(url), false) => {
            ui::step(&ui::dim(&format!("File it at {url}")));
        }
        (None, _) => {
            // Infrastructure and engines have no upstream repository in the profile.
            ui::step(&ui::dim(
                "The profile names no repository for this service, so there is no issue \
                 page to open.",
            ));
        }
    }
    ui::say("");
    Ok(())
}

#[derive(Args, Debug, Clone)]
pub struct DoctorArgs {
    /// Apply the recommended remedy rather than only describing it.
    ///
    /// Only steps Konstruktor can carry out itself — a fixed installer, or starting an
    /// engine that is already installed. Anything needing sudo stays yours to paste.
    #[arg(long)]
    pub fix: bool,
    /// Skip the confirmation. Required when this is not a terminal.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use konstruktor_core::remedy::{InstallerId, Remedy, StartTarget, Step};

    /// `logs -f` never ends on its own, so Ctrl-C is its *success* path — but only when
    /// it really was Ctrl-C. Anything else has to keep surfacing as the failure it is.
    #[cfg(unix)]
    #[test]
    fn only_a_sigint_ends_a_follow_cleanly() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::ExitStatus;

        assert!(was_interrupted(&ExitStatus::from_raw(2)));

        // Killed by something else, or exited with a code — including 130, which a
        // command is free to return for its own reasons.
        assert!(!was_interrupted(&ExitStatus::from_raw(9)));
        assert!(!was_interrupted(&ExitStatus::from_raw(130 << 8)));
        assert!(!was_interrupted(&ExitStatus::from_raw(0)));
        assert!(!was_interrupted(&ExitStatus::from_raw(1 << 8)));
    }

    fn remedy(steps: Vec<Step>) -> Remedy {
        Remedy {
            title: "A remedy".into(),
            body: String::new(),
            steps,
            primary: true,
        }
    }

    /// `--fix` must only offer to act where it actually can. A Linux remedy is a pair of
    /// sudo commands to paste, and treating that as runnable would promise something the
    /// CLI cannot deliver.
    #[test]
    fn only_installer_and_start_steps_count_as_runnable() {
        assert!(has_runnable_step(&remedy(vec![Step::RunInstaller {
            label: "Install".into(),
            installer: InstallerId::BrewColima,
        }])));
        assert!(has_runnable_step(&remedy(vec![Step::StartEngine {
            label: "Start".into(),
            target: StartTarget::Colima,
        }])));

        assert!(!has_runnable_step(&remedy(vec![
            Step::CopyCommand {
                label: "Run this".into(),
                command: "sudo apt-get install podman".into(),
            },
            Step::OpenUrl {
                label: "Read".into(),
                url: "https://example.invalid".into(),
            },
            Step::Note {
                text: "Something to know".into(),
            },
        ])));
        assert!(!has_runnable_step(&remedy(vec![])));
    }
}
