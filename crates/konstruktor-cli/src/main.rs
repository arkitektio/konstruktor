mod create;
mod engine;
mod manage;
mod ui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use konstruktor_core::create::MeshMode;

/// Create and manage Arkitekt hub deployments from a terminal.
///
/// Every command is a thin shell over `konstruktor-core`, which the desktop app links
/// against too — the two front ends run the same code, not merely equivalent code.
#[derive(Parser)]
#[command(
    name = "konstruktor",
    version,
    about = "Create and manage Arkitekt hub deployments.",
    long_about = None,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a hub: generate it, authorize it, write it, start it.
    #[command(subcommand)]
    Hub(HubCommand),
    /// A plugin engine: one deployer container that runs an organization's plugins.
    #[command(subcommand)]
    Engine(EngineCommand),
    /// Report what this hub is and what is running.
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
    /// The containers of a deployment.
    Ps(manage::Target),
    /// A deployment's logs.
    Logs(manage::LogsArgs),
    /// A dev hub's source checkouts: list their branches, or switch to one.
    Checkout(manage::CheckoutArgs),
    /// Create an admin account in one running service.
    Superuser(manage::SuperuserArgs),
    /// Check whether Docker is ready.
    Doctor,
}

#[derive(Subcommand)]
enum HubCommand {
    /// Create a hub.
    Create(Box<create::CreateArgs>),
}

#[derive(Subcommand)]
enum EngineCommand {
    /// Create a plugin engine.
    Create(Box<engine::EngineCreateArgs>),
}

/// Exit codes, so a script can tell the failures apart.
mod exit {
    pub const FAILURE: i32 = 1;
    pub const USAGE: i32 = 2;
    pub const DOCKER: i32 = 3;
    pub const AUTHORIZATION: i32 = 4;
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let code = match run(cli).await {
        Ok(()) => 0,
        Err(error) => {
            ui::fail(&format!("{error:#}"));
            classify(&error)
        }
    };
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
            CreateError::Folder(_) => exit::USAGE,
            _ => exit::FAILURE,
        };
    }
    if error.downcast_ref::<HubAuthorizationError>().is_some() {
        return exit::AUTHORIZATION;
    }
    exit::FAILURE
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Hub(HubCommand::Create(args)) => create::run(*args).await,
        Command::Engine(EngineCommand::Create(args)) => engine::run(*args).await,
        Command::Checkout(args) => manage::checkout(&args),
        Command::Doctor => manage::doctor().await,
        Command::List => manage::list(),
        Command::Status(target) => manage::status(&target).await,
        Command::Up(target) => {
            manage::compose(&target, konstruktor_core::compose::up(), "Starting")
        }
        Command::Stop(target) => {
            manage::compose(&target, konstruktor_core::compose::stop(), "Stopping")
        }
        Command::Down(args) => manage::down(args),
        Command::Pull(target) => {
            manage::compose(&target, konstruktor_core::compose::pull(), "Pulling")
        }
        Command::Ps(target) => manage::ps(&target).await,
        Command::Logs(args) => manage::logs(args),
        Command::Superuser(args) => manage::superuser(args),
    }
}

/// Shared by `hub create` and, later, `authorize`.
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
