use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use konstruktor_core::{compose, create, credentials, docker, git, profile, registry};

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
    let git = git::probe();

    ui::say("");
    let rows = vec![
        (
            "docker".to_string(),
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
        ui::ok("Docker is ready.");
        if !git.is_ready() {
            ui::step(&ui::dim(
                "git is not installed. Hubs do not need it — only a dev hub, which checks \
                 the services' source out and mounts it into the containers, does.",
            ));
        }
        ui::say("");
        Ok(())
    } else {
        // The three failures have three different remedies, worded once in the core.
        Err(anyhow!(create::CreateError::Docker(
            create::describe_docker(&probe)
        )))
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
            ui::say(&format!(
                "    {}",
                ui::dim(&format!("{identifier} at {server}"))
            ));
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
        args.email.as_deref().map(str::trim).filter(|e| !e.is_empty()),
    );

    ui::say("");
    ui::step(&format!(
        "Creating {} in {}…",
        ui::bold(username.trim()),
        ui::bold(&args.service)
    ));

    let output = std::process::Command::new("docker")
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
