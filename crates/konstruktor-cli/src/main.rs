mod authorize;
mod compose_cmd;
mod coord;
mod create;
mod engine;
mod manage;
mod self_cmd;
mod services;
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
    arg_required_else_help = true,
    before_help = banner(),
    help_template = help_template(),
    after_help = AFTER_HELP,
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Emit the answer as JSON on stdout, with no narration mixed into it.
    ///
    /// Global, but only the reporting commands have a document to emit — `status`,
    /// `list`, `ps`, `doctor`, `check`, `update --check`, `rollback`,
    /// `hub services list` and `hub templates`.
    #[arg(long, global = true)]
    json: bool,
}

/// The commands, by what they are for.
///
/// Written out by hand because clap files every subcommand under one heading, and thirty
/// of them in the order they were added tell nobody which create, which only take a hub,
/// and which delete data. The cost is that a new command has to be added here as well —
/// `every_command_is_in_the_help` fails until it is.
const COMMANDS: &str = "\
Create a deployment:
  hub create       A hub: rekuest, mikro, fluss and the rest behind a gateway
  hub templates    List the kinds of hub `hub create --template` makes
  engine create    A plugin engine: one container running an org's plugins
  coord create     A coordination server: what the other two authorize against

Run it (any deployment):
  up               Start it
  stop             Stop its containers, keeping them
  restart          Restart its containers, or one service's
  down             Stop and remove its containers and networks; data is kept
  pull             Download newer images without restarting anything
  status           Show what it is and what is running
  ps               List its containers
  logs             Show its logs (-f to follow)
  list             List the deployments this machine knows about

Change a hub:
  hub services     List, add or remove its services
  authorize        Authorize it again: new addresses, or a mesh key
  update           Update the services whose images changed upstream
  rollback         Return to the images it ran before the last update
  open             Open it in a browser
  superuser        Create an admin account in one service
  checkout         List or switch the branches of a dev hub's source checkouts
  compose          Show, validate, edit or reset its compose file
  hub regenerate   Rewrite its generated files from its profile
  check            Check that every service answers on every advertised address

Change an engine:
  engine attach    Join a hub's network, so its plugins reach the hub in Docker
  engine detach    Leave the hub's network

Back up a hub:
  backup           Back up its database and object storage into a folder
  restore          Restore a backup into it

Troubleshoot:
  doctor           Check whether Docker is ready, and optionally fix it
  report           Write a bug report for one service, secrets removed

Remove (least to most):
  forget           Remove it from `list`; nothing on disk is touched
  purge            Delete its data, keeping the deployment
  destroy          Delete it completely: containers, data, folder, list entry

Konstruktor itself:
  self install     Put konstruktor on your PATH
";

/// clap's own layout, with the banner where the one-line description would go and
/// [`COMMANDS`] where `{subcommands}` would.
fn help_template() -> String {
    format!("{{before-help}}{{usage-heading}} {{usage}}\n\n{COMMANDS}\nOptions:\n{{options}}{{after-help}}")
}

/// The crane, drawn in block characters, three lines of it beside the text.
const CRANE: [&str; 4] = ["▀▜▛▀▀▀▀▀▀▀", " ▐▌     │ ", " ▐▌    ▐█▌", "▗▟▙▖"];

/// What the top-level help opens with: the crane, the version, and where you are.
///
/// The third line is the current directory because that is the deployment every command
/// acts on when none is named — and it says which kind is there, if any, so "here" is
/// never a guess. Passed to clap as `before_help` rather than written into the template:
/// a template is parsed for `{tags}`, and a path is free to contain braces.
fn banner() -> String {
    let here = std::env::current_dir().ok();
    let place = match &here {
        Some(dir) => {
            let shown = match dirs::home_dir().and_then(|home| dir.strip_prefix(home).ok()) {
                Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
                Some(rest) => format!("~/{}", rest.display()),
                None => dir.display().to_string(),
            };
            match konstruktor_core::profile::holds_a_deployment(dir) {
                Some(kind) => format!("{shown} · {}", kind.label()),
                None => shown,
            }
        }
        None => String::new(),
    };
    let text = [
        format!("\x1b[1mKonstruktor\x1b[0m v{}", env!("CARGO_PKG_VERSION")),
        "\x1b[2mBuild and deploy your Arkitekt server\x1b[0m".to_string(),
        format!("\x1b[2m{place}\x1b[0m"),
    ];
    // clap prints through a stream that drops the escapes when stdout is not a terminal.
    let mut out = String::new();
    for (row, crane) in CRANE.iter().enumerate() {
        out.push_str(&format!("\x1b[36m{crane}\x1b[0m"));
        if let Some(line) = text.get(row) {
            out.push_str("   ");
            out.push_str(line);
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// How a deployment is named, which is the one thing every group above shares.
const AFTER_HELP: &str = "\
<target> is a path or a name from `konstruktor list`. Left out, it is the
deployment in the current directory. Every command that takes one also takes
it as --in <target>. The \"hub\" groups above work on hubs only, and say so when
given anything else.

  konstruktor hub create ~/MyHub
  konstruktor hub services add bank --in MyHub
  konstruktor up MyHub
  konstruktor status --json | jq .
";

// In the order of `COMMANDS`, so the source reads the way the help does.
#[derive(Subcommand)]
enum Command {
    // --- create a deployment -----------------------------------------------------
    /// Create a hub, change its services, or regenerate its files.
    #[command(subcommand)]
    Hub(HubCommand),
    /// Create a plugin engine, or attach one to a hub's network.
    #[command(subcommand)]
    Engine(EngineCommand),
    /// Create a coordination server.
    #[command(subcommand)]
    Coord(CoordCommand),

    // --- run it: any deployment --------------------------------------------------
    /// Start a deployment.
    Up(manage::Target),
    /// Stop a deployment's containers, keeping them.
    Stop(manage::Target),
    /// Restart a deployment's containers, or just one service's.
    Restart(manage::RestartArgs),
    /// Stop and remove a deployment's containers and networks, keeping its data.
    Down(manage::DownArgs),
    /// Download newer images for a deployment, without restarting anything.
    Pull(manage::Target),
    /// Show what a deployment is and what is running.
    Status(manage::Target),
    /// List a deployment's containers.
    Ps(manage::Target),
    /// Show a deployment's logs.
    Logs(manage::LogsArgs),
    /// List the deployments this machine knows about.
    List,

    // --- change a hub ------------------------------------------------------------
    /// Authorize a hub again: change the addresses it advertises, or get a mesh key.
    Authorize(Box<authorize::AuthorizeArgs>),
    /// Update the services whose images have changed upstream.
    Update(manage::UpdateArgs),
    /// Return a hub to the images it was running before its last update.
    Rollback(manage::RollbackArgs),
    /// Open a hub in a browser.
    Open(manage::OpenArgs),
    /// Create an admin account in one running service.
    Superuser(manage::SuperuserArgs),
    /// List the branches of a dev hub's source checkouts, or switch to one.
    Checkout(manage::CheckoutArgs),
    /// Show, validate, edit or reset a hub's compose file.
    #[command(subcommand)]
    Compose(compose_cmd::ComposeCommand),
    /// Check that every service answers on every address the hub advertises.
    // `gateway` is what this was called, which read as a command that manages the
    // gateway rather than one that asks through it.
    #[command(alias = "gateway")]
    Check(manage::Target),

    // --- back up a hub -----------------------------------------------------------
    /// Back up a hub's database and object storage into a folder.
    Backup(manage::BackupArgs),
    /// Restore a backup into a hub, then check that its services still answer.
    Restore(manage::RestoreArgs),

    // --- troubleshoot ------------------------------------------------------------
    /// Check whether Docker is ready, and optionally fix it.
    Doctor(manage::DoctorArgs),
    /// Write a bug report for one service: its environment and log, secrets removed.
    Report(manage::ReportArgs),

    // --- remove: three amounts of destruction, least first -----------------------
    /// Remove a deployment from `list`. Nothing on disk is touched.
    Forget(manage::Target),
    /// Delete a deployment's data, keeping the deployment itself.
    Purge(manage::PurgeArgs),
    /// Delete a deployment completely: its containers, its data, its folder, its entry.
    Destroy(manage::DestroyArgs),

    // --- konstruktor itself ------------------------------------------------------
    /// Manage konstruktor itself: `self install` puts it on your PATH.
    #[command(name = "self", subcommand)]
    SelfCmd(self_cmd::SelfCommand),

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
    /// Create a hub: generate it, authorize it, write it, start it.
    Create(Box<create::CreateArgs>),
    /// List the templates `hub create --template` takes.
    Templates,
    /// List a hub's services, add some, or take some out.
    #[command(subcommand)]
    Services(services::ServicesCommand),
    /// Write every generated file again from the hub's profile.
    ///
    /// The compose file, the gateway and the service configs: what brings a hub created
    /// by an older Konstruktor up to the layout this one generates. `compose reset` does
    /// the compose file alone.
    Regenerate(compose_cmd::ConfirmArgs),
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
        Command::Hub(HubCommand::Templates) => create::templates(json),
        Command::Hub(HubCommand::Services(command)) => services::run(command, json).await,
        Command::Hub(HubCommand::Regenerate(args)) => compose_cmd::regenerate_hub(args).await,
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
        Command::Destroy(args) => manage::destroy(args).await,
        Command::Purge(args) => manage::purge(args),
        Command::Forget(target) => manage::forget(&target),
        Command::Report(args) => manage::report(args).await,
        Command::Backup(args) => manage::backup(args).await,
        Command::Restore(args) => manage::restore(args).await,
        Command::HubReport(args) => hub_report(args).await,
        Command::Check(target) => manage::gateway(&target, json).await,
        Command::Compose(command) => compose_cmd::run(command).await,
        Command::SelfCmd(command) => self_cmd::run(command),
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(argv: &[&str]) -> Result<Command, clap::Error> {
        let mut full = vec!["konstruktor"];
        full.extend_from_slice(argv);
        Cli::try_parse_from(full).map(|cli| cli.command)
    }

    #[test]
    fn the_command_tree_is_consistent() {
        Cli::command().debug_assert();
    }

    /// The help lists the commands by hand, so nothing but this keeps a new one from
    /// existing without being shown.
    #[test]
    fn every_command_is_in_the_help() {
        let listed: Vec<&str> = COMMANDS
            .lines()
            .filter(|line| line.starts_with("  "))
            .filter_map(|line| line.split_whitespace().next())
            .collect();
        for command in Cli::command().get_subcommands() {
            let name = command.get_name();
            assert_eq!(
                listed.contains(&name),
                !command.is_hide_set(),
                "`{name}` is {} the help",
                if command.is_hide_set() {
                    "hidden, but in"
                } else {
                    "missing from"
                }
            );
        }
    }

    #[test]
    fn the_help_fits_a_terminal() {
        for line in COMMANDS.lines().chain(AFTER_HELP.lines()) {
            assert!(line.chars().count() <= 80, "too wide: {line}");
        }
        // `to_string` is the help as a pipe sees it: the banner's colours are gone.
        let help = Cli::command().render_help().to_string();
        assert!(
            help.starts_with(&format!(
                "{}   Konstruktor v{}\n",
                CRANE[0],
                env!("CARGO_PKG_VERSION")
            )),
            "{help}"
        );
        assert!(
            help.contains("Change a hub:") && help.contains("--json"),
            "{help}"
        );
    }

    /// The names these had before they were renamed are in scripts and in muscle memory.
    #[test]
    fn the_old_spellings_still_work() {
        assert!(matches!(
            parse(&["gateway", "MyHub"]),
            Ok(Command::Check(_))
        ));
        assert!(matches!(
            parse(&["compose", "restore-backup"]),
            Ok(Command::Compose(compose_cmd::ComposeCommand::Undo(_)))
        ));
        let Ok(Command::Compose(compose_cmd::ComposeCommand::Show(show))) =
            parse(&["compose", "show", "--backup"])
        else {
            panic!("not a show");
        };
        assert!(show.previous);
        let Ok(Command::Authorize(args)) = parse(&["authorize", "--request-auth-key"]) else {
            panic!("not an authorize");
        };
        assert!(args.request_auth_key);
    }

    #[test]
    fn every_command_takes_the_deployment_as_a_flag() {
        let Ok(Command::Up(target)) = parse(&["up", "--in", "MyHub"]) else {
            panic!("not an up");
        };
        assert_eq!(target.given(), Some("MyHub"));
        let Ok(Command::Up(target)) = parse(&["up", "MyHub"]) else {
            panic!("not an up");
        };
        assert_eq!(target.given(), Some("MyHub"));

        let Ok(Command::Logs(logs)) = parse(&["logs", "--in", "MyHub", "-f"]) else {
            panic!("not a logs");
        };
        assert_eq!(logs.target.given(), Some("MyHub"));
        assert!(logs.follow);

        let Ok(Command::Engine(EngineCommand::Detach(detach))) =
            parse(&["engine", "detach", "--in", "plugins"])
        else {
            panic!("not a detach");
        };
        assert_eq!(detach.engine.given(), Some("plugins"));

        // Two answers to one question is a mistake, not a preference.
        assert!(parse(&["up", "MyHub", "--in", "Other"]).is_err());
    }
}
