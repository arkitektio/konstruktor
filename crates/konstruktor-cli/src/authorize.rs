//! `konstruktor authorize`: re-authorize a hub that already exists — to change what it
//! advertises, join a mesh it was created without, or fetch a fresh mesh key after the
//! last one expired unused.
//!
//! Everything is `konstruktor_core::create::reauthorize` — the same call the desktop app's
//! connect screen makes, so a hub re-authorized from a terminal and one re-authorized from
//! the app send the coordination server the same manifest. Whether a mesh key is asked
//! for, and what a mesh-only hub may advertise, is decided there too.

use anyhow::{bail, Result};
use clap::Args;
use konstruktor_core::connect::manifest::AdvertisedHost;
use konstruktor_core::create::{reauthorize, MeshKeyRequest, ReauthorizeAnswers};
use konstruktor_core::{credentials, hosts, profile};
use tokio_util::sync::CancellationToken;

use crate::manage::Target;
use crate::ui;

#[derive(Args, Debug, Clone)]
pub struct AuthorizeArgs {
    #[command(flatten)]
    pub target: Target,
    /// The hub's name inside the organization. Defaults to what it is authorized as now.
    #[arg(long)]
    pub identifier: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    /// An address to advertise. Repeatable. Replaces what the hub advertises now, and
    /// overrides --reach. Ignored for a mesh-only hub.
    #[arg(long = "host")]
    pub hosts: Vec<String>,
    /// Rescan this machine and advertise what matches: local-only · this-network · public.
    ///
    /// Without this and without --host, the hub re-advertises what it already claims. The
    /// mesh address needs neither: it is declared on its own, and the coordination server
    /// fills it in once the hub has joined.
    #[arg(long, value_parser = crate::parse_reach)]
    pub reach: Option<hosts::ReachPresetId>,
    /// Whether to ask for a mesh key: auto (when the hub is not on a mesh yet, or its key
    /// was issued for a login), fresh (always — for a hub whose key expired before it
    /// joined, or whose node the coordination server removed), or never.
    #[arg(long, default_value = "auto", value_parser = parse_mesh_key)]
    pub mesh_key: MeshKeyRequest,
    /// Shorthand for `--mesh-key fresh`.
    #[arg(long)]
    pub request_auth_key: bool,
    /// Do not open a browser for the authorization.
    #[arg(long)]
    pub no_open: bool,
    /// Never prompt.
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

pub async fn run(args: AuthorizeArgs) -> Result<()> {
    let dir = args.target.resolve()?;
    let profile = profile::read_profile(&dir)?;
    let config = &profile.config;
    let existing = credentials::read_credentials(&dir);

    let identifier = match (&args.identifier, &existing) {
        (Some(given), _) => given.clone(),
        (None, Some(creds)) => creds.identifier.clone(),
        // A hub that has never been authorized has no identifier to fall back on, and
        // guessing one would name it something the user never chose.
        (None, None) => bail!(
            "this hub has never been authorized, so there is no identifier to reuse — \
             pass --identifier"
        ),
    };

    let mesh_key = if args.request_auth_key {
        MeshKeyRequest::Fresh
    } else {
        args.mesh_key
    };
    let mesh_only = config.mesh.as_ref().is_some_and(|m| m.enabled && m.mesh_only);
    if mesh_only && (!args.hosts.is_empty() || args.reach.is_some()) {
        ui::warn("This hub is mesh-only: it publishes no port, so --host and --reach are ignored.");
    }
    let hosts = if mesh_only {
        Vec::new()
    } else {
        resolve_hosts(&args, existing.as_ref()).await?
    };

    ui::say("");
    ui::say(&format!("  {}", ui::bold("Authorizing a hub")));
    ui::say("");
    ui::table(&[
        ("folder".into(), dir.to_string_lossy().to_string()),
        ("coordination".into(), config.coord_server.clone()),
        ("identifier".into(), identifier.clone()),
        (
            "advertised".into(),
            if mesh_only {
                "the mesh only — no ports opened here".into()
            } else {
                hosts.iter().map(|h| h.host.as_str()).collect::<Vec<_>>().join(", ")
            },
        ),
        (
            "mesh key".into(),
            if mesh_key.asks(config) {
                "requested".into()
            } else {
                "not requested".into()
            },
        ),
    ]);
    ui::say("");

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

    let answers = ReauthorizeAnswers {
        dir: dir.clone(),
        coord_server: config.coord_server.clone(),
        identifier,
        description: args.description.clone(),
        hosts,
        // The CLI has no prober: marking an address externally reachable is a claim only
        // something outside this machine can make.
        reachable_hosts: Vec::new(),
        mesh_key,
    };

    let cancel = CancellationToken::new();
    let on_signal = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            on_signal.cancel();
        }
    });

    let open_browser = !args.no_open && ui::is_interactive();
    let done = reauthorize(&answers, &cancel, &|event| {
        crate::create::report(event, open_browser)
    })
    .await?;

    ui::say("");
    ui::ok(&format!(
        "Authorized as {} at {}.",
        done.credentials.identifier, done.credentials.server
    ));
    // Nothing was started: a granted key's fifteen minutes run from now.
    crate::create::report_outcome(
        done.mesh_requested,
        done.mesh_granted,
        done.reporter_enabled,
        false,
    );
    ui::step(&ui::dim(
        "The service configs were regenerated — `konstruktor up` restarts the stack \
         against them.",
    ));
    ui::say("");
    Ok(())
}

/// What to advertise: an explicit list, a fresh scan, or what the hub already claims.
async fn resolve_hosts(
    args: &AuthorizeArgs,
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

    match existing.map(|c| c.advertised_hosts.clone()) {
        Some(hosts) if !hosts.is_empty() => Ok(hosts),
        _ => bail!(
            "this hub does not record what it advertises, so there is nothing to reuse — \
             pass --host, or --reach to scan this machine"
        ),
    }
}
