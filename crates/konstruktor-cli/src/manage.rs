use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use konstruktor_core::{compose, create, credentials, docker, profile, registry};

use crate::ui;

/// A deployment to act on: a path, a registered name, or — when neither is given — the
/// current directory, if it holds a hub.
#[derive(Args, Debug, Clone)]
pub struct Target {
    /// A path, or the name of a registered deployment.
    pub target: Option<String>,
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
pub struct LogsArgs {
    #[command(flatten)]
    pub target: Target,
    /// Only this service.
    #[arg(long)]
    pub service: Option<String>,
    #[arg(long, default_value_t = 200)]
    pub tail: u32,
}

impl Target {
    pub fn resolve(&self) -> Result<PathBuf> {
        let store = registry::load();

        match &self.target {
            Some(given) => {
                let as_path = PathBuf::from(given);
                if profile::holds_a_hub(&as_path) {
                    return Ok(as_path);
                }
                if let Some(record) = registry::find_by_name(&store, given) {
                    let path = PathBuf::from(&record.path);
                    if profile::holds_a_hub(&path) {
                        return Ok(path);
                    }
                    // A registered deployment whose folder has since been deleted or
                    // moved. Saying so beats a bare "no such file", which reads as a bug.
                    bail!(
                        "`{given}` is registered at {}, but there is no hub there any more — it was moved or deleted.",
                        record.path
                    );
                }
                if as_path.exists() {
                    bail!("{given} does not hold a hub deployment");
                }
                bail!("no deployment called `{given}`, and no folder at that path")
            }
            None => {
                let here = std::env::current_dir().context("reading the current directory")?;
                if profile::holds_a_hub(&here) {
                    return Ok(here);
                }
                bail!(
                    "this directory holds no hub. Name one — `konstruktor list` shows what \
                     is registered — or give a path."
                )
            }
        }
    }
}

pub async fn doctor() -> Result<()> {
    let probe = docker::probe().await;

    ui::say("");
    let rows = vec![
        (
            "docker".to_string(),
            probe.cli_version.clone().unwrap_or_else(|| "not found".into()),
        ),
        (
            "compose".to_string(),
            probe.compose_version.clone().unwrap_or_else(|| "not found".into()),
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
    ];
    ui::table(&rows);
    ui::say("");

    if probe.is_ready() {
        ui::ok("Docker is ready.");
        ui::say("");
        Ok(())
    } else {
        // The three failures have three different remedies, worded once in the core.
        Err(anyhow!(create::CreateError::Docker(create::describe_docker(
            &probe
        ))))
    }
}

pub fn list() -> Result<()> {
    let store = registry::load();

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
            ui::say(&format!("    {}", ui::dim(&format!("{identifier} at {server}"))));
        }
    }
    ui::say("");
    Ok(())
}

pub async fn status(target: &Target) -> Result<()> {
    let dir = target.resolve()?;
    let profile = profile::read_profile(&dir)?;
    let config = &profile.config;

    ui::say("");
    let mut rows = vec![
        ("folder".into(), dir.to_string_lossy().to_string()),
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
            config
                .enabled_services()
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ),
        (
            "ports".into(),
            format!(
                "{} / {}",
                config.gateway.exposed_http_port.unwrap_or(80),
                config.gateway.exposed_https_port.unwrap_or(443)
            ),
        ),
    ];

    match &config.mesh {
        Some(mesh) if mesh.enabled => {
            rows.push(("mesh".into(), format!("joins as {}", mesh.hostname)))
        }
        _ => rows.push(("mesh".into(), "not joined".into())),
    }

    match credentials::read_credentials(&dir) {
        Some(creds) => rows.push((
            "authorized".into(),
            format!("{} as {}", creds.authorized_at, creds.identifier),
        )),
        None => rows.push(("authorized".into(), "not yet".into())),
    }

    ui::table(&rows);

    // Container state is a nice-to-have: a stopped daemon must not fail `status`.
    match docker::list_deployment_containers(&dir.to_string_lossy()).await {
        Ok(containers) if !containers.is_empty() => {
            ui::say("");
            for container in containers {
                let name = container.service.unwrap_or_else(|| "?".into());
                let state = container.state.unwrap_or_else(|| "unknown".into());
                ui::say(&format!("  {:16}  {}", name, ui::dim(&state)));
            }
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

    ui::say("");
    Ok(())
}

/// Runs a compose subcommand in the deployment folder, streaming its output through.
pub fn compose(target: &Target, args: Vec<&str>, verb: &str) -> Result<()> {
    let dir = target.resolve()?;
    ui::say("");
    ui::step(&format!("{verb} {}…", ui::bold(&dir.to_string_lossy())));

    let status = std::process::Command::new("docker")
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
        let dir = args.target.resolve()?;
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

pub async fn ps(target: &Target) -> Result<()> {
    let dir = target.resolve()?;
    let status = std::process::Command::new("docker")
        .args(compose::ps())
        .current_dir(&dir)
        .status()
        .context("running docker")?;
    if !status.success() {
        bail!("docker compose ps exited with {status}");
    }
    Ok(())
}

pub fn logs(args: LogsArgs) -> Result<()> {
    let dir = args.target.resolve()?;
    let argv = compose::logs(args.service.as_deref(), args.tail);

    let status = std::process::Command::new("docker")
        .args(&argv)
        .current_dir(&dir)
        .status()
        .context("running docker")?;
    if !status.success() {
        bail!("docker {} exited with {status}", argv.join(" "));
    }
    Ok(())
}
