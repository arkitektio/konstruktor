//! `konstruktor hub connect` and `hub disconnect`: a hub joining a coordination server's
//! organizations, and leaving them.
//!
//! A hub is created unconnected: it names itself, mints its own keys and trusts its own
//! services, and takes nobody's tokens. Connecting sends what it is to a coordination server,
//! and somebody accepts it into an organization; connecting again is how it is accepted into
//! another one, joins the mesh, or moves to new addresses. Its services take the tokens of
//! exactly the organizations that accepted it.
//!
//! Everything is `konstruktor_core::create::connect` — the same call the desktop app's
//! connect screen makes, so a hub connected from a terminal and one connected from the app
//! send the coordination server the same manifest. `konstruktor authorize` is the old name.

use anyhow::{bail, Result};
use clap::Args;
use konstruktor_core::connect::manifest::AdvertisedHost;
use konstruktor_core::create::{self, ConnectAnswers, MeshKeyRequest};
use konstruktor_core::{credentials, hosts, profile};
use tokio_util::sync::CancellationToken;

use crate::manage::Target;
use crate::ui;

#[derive(Args, Debug, Clone)]
pub struct ConnectArgs {
    #[command(flatten)]
    pub target: Target,
    /// The coordination server, e.g. go.arkitekt.live. Defaults to the one the hub is
    /// connected to: a hub is connected to one, in as many of its organizations as accept it.
    #[arg(long)]
    pub server: Option<String>,
    /// What the hub is called there. Defaults to what it was created as.
    #[arg(long)]
    pub identifier: Option<String>,
    /// A line about the hub, sent with the request.
    #[arg(long)]
    pub description: Option<String>,
    /// An address to advertise. Repeatable.
    ///
    /// Replaces what the hub advertises now, and overrides --reach. Ignored for a
    /// mesh-only hub.
    #[arg(long = "host", value_name = "HOST")]
    pub hosts: Vec<String>,
    /// Rescan this machine and advertise what matches: local-only · this-network · public.
    ///
    /// Without this and without --host, the hub advertises what it was created with. The
    /// mesh address needs neither: it is declared on its own, and the coordination server
    /// fills it in once the hub has joined.
    #[arg(long, value_parser = crate::parse_reach)]
    pub reach: Option<hosts::ReachPresetId>,
    /// Whether to ask for a mesh key: auto · fresh · never.
    ///
    /// auto asks when the hub wants a mesh it is not on yet, or its key was issued for a
    /// login. fresh always asks — to join a mesh a hub was created without, for a hub
    /// whose key expired before it joined, or whose node the coordination server removed.
    #[arg(long, default_value = "auto", value_parser = parse_mesh_key)]
    pub mesh_key: MeshKeyRequest,
    /// Shorthand for `--mesh-key fresh`. Kept for scripts that already pass it; the
    /// help shows the one spelling.
    #[arg(long, hide = true)]
    pub request_auth_key: bool,
    /// Do not open a browser for the acceptance.
    #[arg(long)]
    pub no_open: bool,
    /// Answer yes to the confirmation. It is never asked when this is not a terminal.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Args, Debug, Clone)]
pub struct DisconnectArgs {
    #[command(flatten)]
    pub target: Target,
    /// The organization to leave, as `konstruktor status` lists them. Needed only when the
    /// hub is connected to more than one.
    #[arg(long = "org", value_name = "ORGANIZATION")]
    pub organization: Option<String>,
    /// Answer yes to the confirmation. It is never asked when this is not a terminal.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

fn parse_mesh_key(value: &str) -> Result<MeshKeyRequest, String> {
    match value {
        "auto" => Ok(MeshKeyRequest::Auto),
        "fresh" => Ok(MeshKeyRequest::Fresh),
        "never" => Ok(MeshKeyRequest::Never),
        other => Err(format!("`{other}` is not one of auto, fresh, never")),
    }
}

pub async fn run(args: ConnectArgs) -> Result<()> {
    let dir = args.target.resolve()?;
    connect(&dir, &args).await
}

/// Connects the hub at `dir`, as `args` say: what `hub connect` does, and what `hub create
/// --server` does once the hub is written.
pub(crate) async fn connect(dir: &std::path::Path, args: &ConnectArgs) -> Result<()> {
    let config = profile::read_profile(dir)?.config;
    let existing = credentials::read_credentials(dir);

    let Some(server) = args
        .server
        .clone()
        .or_else(|| existing.as_ref().map(|held| held.server.clone()))
        .filter(|server| !server.trim().is_empty())
    else {
        bail!("this hub is not connected anywhere yet — pass --server, e.g. --server go.arkitekt.live");
    };
    if let Some(held) = &existing {
        if held.server.trim() != server.trim() {
            bail!(
                "this hub is connected to {} — `konstruktor hub disconnect` it from every \
                 organization there before connecting it to {server}",
                held.server
            );
        }
    }

    let Some(identifier) = args
        .identifier
        .clone()
        .or_else(|| config.identifier.clone())
        .or_else(|| existing.as_ref().map(|held| held.identifier.clone()))
    else {
        // A name nobody chose is not one to send.
        bail!("this hub was created before it kept a name to connect as — pass --identifier");
    };

    let mesh_key = if args.request_auth_key {
        MeshKeyRequest::Fresh
    } else {
        args.mesh_key
    };
    let mesh_only = config.mesh.as_ref().is_some_and(|m| m.mesh_only);
    if mesh_only && (!args.hosts.is_empty() || args.reach.is_some()) {
        ui::warn("This hub is mesh-only: it publishes no port, so --host and --reach are ignored.");
    }
    let hosts = if mesh_only {
        Vec::new()
    } else {
        resolve_hosts(args, &config, existing.as_ref()).await?
    };
    let organizations = existing
        .as_ref()
        .map(|held| held.organizations())
        .unwrap_or_default();

    ui::say("");
    ui::say(&format!("  {}", ui::bold("Connecting a hub")));
    ui::say("");
    ui::table(&[
        ("folder".into(), dir.to_string_lossy().to_string()),
        ("coordination".into(), server.clone()),
        (
            "connected to".into(),
            if organizations.is_empty() {
                "no organization yet".into()
            } else {
                format!("organization {}", organizations.join(", "))
            },
        ),
        ("identifier".into(), identifier.clone()),
        (
            "advertised".into(),
            if mesh_only {
                "the mesh only — no ports opened here".into()
            } else {
                hosts
                    .iter()
                    .map(|h| h.host.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            },
        ),
        (
            "mesh key".into(),
            if mesh_key.asks(&config) {
                "requested".into()
            } else {
                "not requested".into()
            },
        ),
    ]);
    ui::say("");
    if !organizations.is_empty() {
        ui::step(&ui::dim(
            "Accepted into an organization it is connected to, the hub replaces what that \
             organization knew of it; into another, it is connected to that one too.",
        ));
        ui::say("");
    }

    if !args.yes && ui::is_interactive() {
        let confirmed = inquire::Confirm::new("Send this to the coordination server?")
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

    let answers = ConnectAnswers {
        dir: dir.to_path_buf(),
        coord_server: server,
        identifier,
        description: args.description.clone().or(config.description.clone()),
        hosts,
        // The CLI has no prober: marking an address externally reachable is a claim only
        // something outside this machine can make.
        reachable_hosts: Vec::new(),
        mesh_key,
        // `konstruktor hub services` changes them, through this same call.
        services: None,
        described: Default::default(),
    };

    let cancel = CancellationToken::new();
    let on_signal = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            on_signal.cancel();
        }
    });

    let open_browser = !args.no_open && ui::is_interactive();
    let done = create::connect(&answers, &cancel, &|event| {
        crate::create::report(event, open_browser)
    })
    .await?;

    ui::say("");
    ui::ok(&match &done.organization {
        Some(organization) => format!(
            "Connected to organization {organization} at {}, as {}.",
            done.credentials.server, done.credentials.identifier
        ),
        None => format!(
            "Connected at {}, as {}.",
            done.credentials.server, done.credentials.identifier
        ),
    });
    let others: Vec<String> = done
        .credentials
        .organizations()
        .into_iter()
        .filter(|org| Some(org) != done.organization.as_ref())
        .collect();
    if !others.is_empty() {
        ui::step(&format!(
            "Also connected to organization {}.",
            others.join(", ")
        ));
    }
    let started = restart_if_running(dir).await?;
    crate::create::report_outcome(
        done.mesh_requested,
        done.mesh_granted,
        done.reporter_enabled,
        started,
    );
    ui::say("");
    Ok(())
}

pub async fn disconnect(args: DisconnectArgs) -> Result<()> {
    let dir = args.target.resolve()?;
    let Some(held) = credentials::read_credentials(&dir) else {
        bail!("this hub is not connected to any organization");
    };
    let organizations = held.organizations();
    let organization = match (&args.organization, organizations.as_slice()) {
        (Some(given), _) => given.clone(),
        (None, [only]) => only.clone(),
        (None, []) => bail!(
            "this hub's grant does not say which organization accepted it — reconnect it \
             once, so it does"
        ),
        (None, many) => bail!(
            "this hub is connected to organizations {} — say which to leave with --org",
            many.join(", ")
        ),
    };

    ui::say("");
    ui::say(&format!("  {}", ui::bold("Disconnecting a hub")));
    ui::say("");
    ui::table(&[
        ("folder".into(), dir.to_string_lossy().to_string()),
        ("coordination".into(), held.server.clone()),
        ("organization".into(), organization.clone()),
    ]);
    ui::say("");
    let last = organizations.len() <= 1;
    ui::step(&ui::dim(if last {
        "It is the hub's only organization: afterwards its services take nobody's tokens \
         until it is connected again."
    } else {
        "The hub stays connected to its other organizations."
    }));
    ui::say("");
    if !args.yes && ui::is_interactive() {
        let confirmed = inquire::Confirm::new("Ask the coordination server to let go of it?")
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

    let done = create::disconnect(&dir, &organization).await?;
    ui::say("");
    ui::ok(&format!("Disconnected from organization {organization}."));
    if done.organizations.is_empty() {
        ui::step("Connected nowhere now.");
    } else {
        ui::step(&format!(
            "Still connected to organization {}.",
            done.organizations.join(", ")
        ));
    }
    restart_if_running(&dir).await?;
    ui::say("");
    Ok(())
}

/// Starts a running hub again, so its services read the configs just written; leaves a
/// stopped one for its next start. Answers whether it started it.
async fn restart_if_running(dir: &std::path::Path) -> Result<bool> {
    if !konstruktor_core::compose::is_up(dir).await {
        ui::step(&ui::dim(
            "The service configs were written again — the next `konstruktor up` starts the \
             stack on them.",
        ));
        return Ok(false);
    }
    ui::step("Starting the services again on their new configs…");
    let print = |line: konstruktor_core::compose::ComposeLine| {
        if line.stderr {
            eprintln!("  {}", ui::dim(&line.line));
        } else {
            println!("  {}", line.line);
        }
    };
    let report = konstruktor_core::start::start(dir, &print).await?;
    for warning in &report.warnings {
        ui::warn(warning);
    }
    Ok(true)
}

/// What to advertise: an explicit list, a fresh scan, or what the hub was created with.
async fn resolve_hosts(
    args: &ConnectArgs,
    config: &konstruktor_core::config::hub::HubConfig,
    existing: Option<&credentials::HubCredentials>,
) -> Result<Vec<AdvertisedHost>> {
    if !args.hosts.is_empty() {
        return Ok(args
            .hosts
            .iter()
            .map(|host| AdvertisedHost {
                host: host.clone(),
                kind: hosts::classify_host(host),
            })
            .collect());
    }

    if let Some(reach) = args.reach {
        let chosen = hosts::discover(reach).await;
        if chosen.is_empty() {
            bail!("nothing on this machine matches --reach — widen it, or pass --host");
        }
        return Ok(chosen);
    }

    let recorded = if config.advertised_hosts.is_empty() {
        existing
            .map(|held| held.advertised_hosts.clone())
            .unwrap_or_default()
    } else {
        config.advertised_hosts.clone()
    };
    if recorded.is_empty() {
        bail!(
            "this hub does not record what it advertises, so there is nothing to reuse — \
             pass --host, or --reach to scan this machine"
        );
    }
    Ok(recorded)
}
