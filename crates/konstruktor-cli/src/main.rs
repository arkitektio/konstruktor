mod authorize;
mod compose_cmd;
mod coord;
mod create;
mod engine;
mod manage;
mod ui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use konstruktor_core::create::MeshMode;

/// Create and manage Arkitekt deployments from a terminal.
///
/// Every command is a thin shell over `konstruktor-core`, which the desktop app links
/// against too — the two front ends run the same code, not merely equivalent code.
#[derive(Parser)]
#[command(
    name = "konstruktor",
    version,
    about = "Create and manage Arkitekt deployments.",
    long_about = None,
    disable_help_subcommand = true,
    after_help = AFTER_HELP,
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Emit the answer as JSON on stdout, with no narration mixed into it.
    ///
    /// Global, but only the reporting commands have a document to emit — `status`,
    /// `list`, `ps`, `doctor`, `update --check` and `rollback`.
    #[arg(long, global = true)]
    json: bool,
}

/// Konstruktor deploys three things, and the rest of the commands work on all of them.
///
/// Spelling that out here rather than leaving it to be inferred from an alphabetical list:
/// which of the three you have decides only how you *create* it. Once it exists it is a
/// deployment like any other, and `up`, `logs` and `destroy` do not care which kind it is.
const AFTER_HELP: &str = "\
What you can create:
  hub      the services — rekuest, mikro, fluss and the rest — behind a gateway
  engine   a plugin engine: one deployer container running an organization's plugins
  coord    a coordination server: where users, organizations and permissions live,
           and what a hub or an engine authorizes against

Everything else takes any of the three. `[target]` is a path or a registered name;
left out, it is the deployment you are standing in.

  konstruktor hub create ~/MyHub
  konstruktor up ~/MyHub
  konstruktor status --json | jq .
";

#[derive(Subcommand)]
enum Command {
    // --- creating a deployment ---------------------------------------------------
    /// Create a hub: generate it, authorize it, write it, start it.
    #[command(subcommand)]
    Hub(HubCommand),
    /// A plugin engine: one deployer container that runs an organization's plugins.
    #[command(subcommand)]
    Engine(EngineCommand),
    /// A coordination server: what hubs and engines authorize against.
    #[command(subcommand)]
    Coord(CoordCommand),

    // --- everything else, on any of the three ------------------------------------
    /// Re-authorize an existing hub: change what it advertises, or claim a mesh key.
    Authorize(Box<authorize::AuthorizeArgs>),
    /// Report what a deployment is and what is running.
    Status(manage::Target),
    /// The deployments this machine knows about.
    List,
    /// Start a deployment.
    Up(manage::Target),
    /// Stop a deployment's containers, leaving them in place.
    Stop(manage::Target),
    /// Remove a deployment's containers and networks.
    Down(manage::DownArgs),
    /// Pull newer images for a deployment.
    Pull(manage::Target),
    /// Update the services whose images have actually moved upstream.
    Update(manage::UpdateArgs),
    /// Put a hub back on the images it was running before its last update.
    Rollback(manage::RollbackArgs),
    /// The containers of a deployment.
    Ps(manage::Target),
    /// A deployment's logs.
    Logs(manage::LogsArgs),
    /// A dev hub's source checkouts: list their branches, or switch to one.
    Checkout(manage::CheckoutArgs),
    /// Create an admin account in one running service.
    Superuser(manage::SuperuserArgs),
    /// Back a hub's database and object storage up into a folder.
    Backup(manage::BackupArgs),
    /// Restore a backup into a hub, then check that its services still answer.
    Restore(manage::RestoreArgs),
    /// Restart a deployment's containers, or just one service's.
    Restart(manage::RestartArgs),
    /// Open a hub in a browser.
    Open(manage::OpenArgs),
    /// Remove a deployment completely: its containers, its data, its folder, its entry.
    Destroy(manage::DestroyArgs),
    /// Delete a deployment's data, keeping the deployment itself.
    Purge(manage::DestroyArgs),
    /// Stop listing a deployment. Nothing on disk is touched.
    Forget(manage::Target),
    /// A bug report for one service: its environment and its log, with secrets removed.
    Report(manage::ReportArgs),
    /// Check whether Docker is ready, and optionally fix it.
    Doctor(manage::DoctorArgs),
    /// Ask every service through every address the hub advertises, from this machine.
    Gateway(manage::Target),
    /// The compose file by hand: show, validate, edit, reset to the generated one.
    #[command(subcommand)]
    Compose(compose_cmd::ComposeCommand),

    /// Report this hub's health to its coordination server, forever. What the stack's
    /// `reporter` container runs; not for people.
    #[command(hide = true)]
    HubReport(HubReportArgs),
}

#[derive(clap::Args)]
struct HubReportArgs {
    /// Holds `hub_credentials.json` and `hub_config.yaml`.
    #[arg(long, default_value = "/seed")]
    seed: std::path::PathBuf,
    /// Where the rotating refresh token is kept.
    #[arg(long, default_value = "/state")]
    state: std::path::PathBuf,
    /// The gateway, as the stack's own network names it.
    #[arg(long, default_value = konstruktor_core::hubhealth::GATEWAY_BASE)]
    gateway: String,
    /// The tailscale sidecar's LocalAPI socket. Absent on a hub without a mesh.
    #[arg(long, default_value = konstruktor_core::hubhealth::TAILSCALE_SOCKET)]
    tailscale_socket: std::path::PathBuf,
}

async fn hub_report(args: HubReportArgs) -> anyhow::Result<()> {
    use konstruktor_core::hubhealth::{run, ReporterConfig};
    let config = ReporterConfig {
        seed_dir: args.seed,
        state_dir: args.state,
        gateway: args.gateway,
        tailscale_socket: args.tailscale_socket,
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    // Plain lines on stdout: this is read through `docker compose logs`, not a terminal.
    run(&config, &|line| println!("{line}")).await?;
    Ok(())
}

#[derive(Subcommand)]
enum HubCommand {
    /// Create a hub.
    Create(Box<create::CreateArgs>),
}

#[derive(Subcommand)]
enum CoordCommand {
    /// Create a coordination server.
    Create(Box<coord::CoordCreateArgs>),
}

#[derive(Subcommand)]
enum EngineCommand {
    /// Create a plugin engine.
    Create(Box<engine::EngineCreateArgs>),
    /// Join a hub's network, so the engine and its plugins reach it from inside Docker.
    Attach(engine::EngineAttachArgs),
    /// Leave the hub's network; plugins run on the engine's own network again.
    Detach(engine::EngineDetachArgs),
}

/// Exit codes, so a script can tell the failures apart.
mod exit {
    pub const FAILURE: i32 = 1;
    pub const USAGE: i32 = 2;
    pub const DOCKER: i32 = 3;
    pub const AUTHORIZATION: i32 = 4;
}

/// The runtime runs on a thread of its own, with room to spare.
///
/// `run` is one state machine holding every command's, so it is as large as the largest —
/// creating a hub, authorizing it, an update with its backup and health check — and an
/// unoptimized build builds it on the stack before it can be moved anywhere. Windows gives
/// the main thread 1 MiB, which that outgrew: `status` overflowed before it printed a line.
const STACK: usize = 16 * 1024 * 1024;

fn main() {
    let cli = Cli::parse();

    let code = std::thread::Builder::new()
        .name("konstruktor".into())
        .stack_size(STACK)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(STACK)
                .build()
                .expect("a tokio runtime");
            runtime.block_on(async {
                match run(cli).await {
                    Ok(()) => 0,
                    Err(error) => {
                        ui::fail(&format!("{error:#}"));
                        classify(&error)
                    }
                }
            })
        })
        .expect("the main thread")
        .join()
        .unwrap_or(exit::FAILURE);
    std::process::exit(code);
}

/// Maps a failure onto the exit code a script would branch on.
fn classify(error: &anyhow::Error) -> i32 {
    use konstruktor_core::connect::authorize::HubAuthorizationError;
    use konstruktor_core::create::CreateError;

    if let Some(create) = error.downcast_ref::<CreateError>() {
        return match create {
            CreateError::Docker(_) => exit::DOCKER,
            CreateError::Authorization(_) => exit::AUTHORIZATION,
            // An engine's claim failing is the same kind of failure as a hub's, and a
            // script branching on the exit code should not have to tell them apart.
            CreateError::AppAuthorization(_) => exit::AUTHORIZATION,
            // After the grant, not before it: the coordination server said no to the mesh.
            CreateError::NoMeshKey | CreateError::EngineNoMeshKey => exit::AUTHORIZATION,
            CreateError::Folder(_) | CreateError::Answers(_) => exit::USAGE,
            _ => exit::FAILURE,
        };
    }
    if error.downcast_ref::<HubAuthorizationError>().is_some() {
        return exit::AUTHORIZATION;
    }
    if error
        .downcast_ref::<konstruktor_core::connect::app::AppAuthorizationError>()
        .is_some()
    {
        return exit::AUTHORIZATION;
    }
    exit::FAILURE
}

async fn run(cli: Cli) -> Result<()> {
    let json = cli.json;
    match cli.command {
        Command::Hub(HubCommand::Create(args)) => create::run(*args).await,
        Command::Engine(EngineCommand::Create(args)) => engine::run(*args).await,
        Command::Engine(EngineCommand::Attach(args)) => {
            engine::attach(args.engine, Some(args.hub)).await
        }
        Command::Engine(EngineCommand::Detach(args)) => engine::attach(args.engine, None).await,
        Command::Coord(CoordCommand::Create(args)) => coord::run(*args).await,
        Command::Authorize(args) => authorize::run(*args).await,
        Command::Checkout(args) => manage::checkout(&args),
        Command::Doctor(args) => manage::doctor(json, args.fix, args.yes).await,
        Command::List => manage::list(json),
        Command::Status(target) => manage::status(&target, json).await,
        Command::Up(target) => manage::up(&target).await,
        Command::Stop(target) => {
            manage::compose(&target, konstruktor_core::compose::stop(), "Stopping")
        }
        Command::Down(args) => manage::down(args),
        Command::Pull(target) => {
            manage::compose(&target, konstruktor_core::compose::pull(), "Pulling")
        }
        Command::Update(args) => manage::update(args, json).await,
        Command::Rollback(args) => manage::rollback(args, json).await,
        Command::Ps(target) => manage::ps(&target, json).await,
        Command::Logs(args) => manage::logs(args),
        Command::Superuser(args) => manage::superuser(args).await,
        Command::Restart(args) => manage::restart(args).await,
        Command::Open(args) => manage::open(args),
        Command::Destroy(args) => manage::destroy(args),
        Command::Purge(args) => manage::purge(args),
        Command::Forget(target) => manage::forget(&target),
        Command::Report(args) => manage::report(args).await,
        Command::Backup(args) => manage::backup(args).await,
        Command::Restore(args) => manage::restore(args).await,
        Command::HubReport(args) => hub_report(args).await,
        Command::Gateway(target) => manage::gateway(&target, json).await,
        Command::Compose(command) => compose_cmd::run(command).await,
    }
}

/// Shared by `hub create` and `authorize`.
/// How far a hub should reach, as `--reach` spells it.
pub fn parse_reach(value: &str) -> Result<konstruktor_core::hosts::ReachPresetId, String> {
    use konstruktor_core::hosts::ReachPresetId;
    match value {
        "local-only" => Ok(ReachPresetId::LocalOnly),
        "this-network" => Ok(ReachPresetId::ThisNetwork),
        "public" => Ok(ReachPresetId::Public),
        other => Err(format!(
            "unknown reach `{other}` — expected local-only, this-network or public"
        )),
    }
}

pub fn parse_mesh_mode(value: &str) -> Result<MeshMode, String> {
    match value {
        "none" => Ok(MeshMode::None),
        "coordination" => Ok(MeshMode::Coordination),
        "manual" => Ok(MeshMode::Manual),
        other => Err(format!(
            "unknown mesh mode `{other}` — expected none, coordination or manual"
        )),
    }
}
