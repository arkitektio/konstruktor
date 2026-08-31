//! `konstruktor authorize`: re-authorize a hub that already exists.
//!
//! The seam this fills was left open on purpose — `main.rs` has said "Shared by `hub
//! create` and, later, `authorize`" since `parse_reach` was written, and `hub create`
//! ends by telling a mesh hub to "authorize again to advertise" the address the mesh gave
//! it. Until now there was nothing to run.
//!
//! Everything is `konstruktor_core::create::reauthorize` — the same call the desktop app's
//! connect screen makes, so a hub re-authorized from a terminal and one re-authorized from
//! the app send the coordination server the same manifest.

use anyhow::{bail, Result};
use clap::Args;
use konstruktor_core::connect::manifest::AdvertisedHost;
use konstruktor_core::create::{reauthorize, ReauthorizeAnswers};
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
    /// overrides --reach.
    #[arg(long = "host")]
    pub hosts: Vec<String>,
    /// Rescan this machine and advertise what matches: local-only · this-network · public.
    ///
    /// Without this and without --host, the hub re-advertises exactly what it already
    /// claims — which is the point after a mesh grant, since the tailnet address is one
    /// a scan of this machine cannot find on its own.
    #[arg(long, value_parser = crate::parse_reach)]
    pub reach: Option<hosts::ReachPresetId>,
    /// Ask the coordination server for a mesh key as part of this authorization.
    #[arg(long)]
    pub request_auth_key: bool,
    /// Do not open a browser for the authorization.
    #[arg(long)]
    pub no_open: bool,
    /// Never prompt.
    #[arg(long, short = 'y')]
    pub yes: bool,
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

    let hosts = resolve_hosts(&args, config, existing.as_ref()).await?;

    ui::say("");
    ui::say(&format!("  {}", ui::bold("Authorizing a hub")));
    ui::say("");
    let mut rows = vec![
        ("folder".into(), dir.to_string_lossy().to_string()),
        ("coordination".into(), config.coord_server.clone()),
        ("identifier".into(), identifier.clone()),
        (
            "advertised".into(),
            hosts
                .iter()
                .map(|h| h.host.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ),
    ];
    if args.request_auth_key {
        rows.push(("mesh key".into(), "requested".into()));
    }
    ui::table(&rows);
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
        request_auth_key: args.request_auth_key,
    };

    let cancel = CancellationToken::new();
    let on_signal = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            on_signal.cancel();
        }
    });

    let open_browser = !args.no_open && ui::is_interactive();
    let granted = reauthorize(&answers, &cancel, &|event| {
        crate::create::report(event, open_browser)
    })
    .await?;

    ui::say("");
    ui::ok(&format!(
        "Authorized as {} at {}.",
        granted.identifier, granted.server
    ));
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
        // Unlike `create`, this hub may already be on a mesh — so a tailnet address on
        // this machine can now be recognised as *its own* rather than a stranger's.
        let mesh = config
            .mesh
            .as_ref()
            .filter(|m| m.enabled)
            .map(|m| hosts::KnownMesh {
                domain: None,
                hostname: Some(m.hostname.clone()),
            })
            .unwrap_or_default();

        let candidates =
            hosts::host_candidates(&hosts::bindings().await.unwrap_or_default(), &mesh);
        let chosen: Vec<AdvertisedHost> = candidates
            .iter()
            .filter(|c| c.usable && reach.accepts(c.kind))
            .map(|c| AdvertisedHost {
                host: c.value.clone(),
                kind: c.kind,
            })
            .collect();
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
