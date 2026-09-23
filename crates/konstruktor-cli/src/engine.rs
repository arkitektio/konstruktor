use anyhow::{bail, Context, Result};
use clap::Args;
use konstruktor_core::create::identifier_from_folder;
use konstruktor_core::engine::{create_engine, EngineAnswers, DEPLOYER_IMAGE};

use tokio_util::sync::CancellationToken;

use crate::ui;

/// `konstruktor engine create`: the second path, next to `hub create`.
///
/// Far fewer questions than a hub, because an engine is one container: no services, no
/// ports, no addresses to advertise, no mesh.
#[derive(Args, Debug, Clone)]
pub struct EngineCreateArgs {
    /// Where the engine lives. Defaults to the current directory.
    pub dir: Option<String>,
    /// How this deployment is labelled. Defaults to the folder's name.
    #[arg(long)]
    pub name: Option<String>,
    /// The coordination server this engine configures itself against.
    #[arg(long)]
    pub server: Option<String>,
    /// The engine's name inside the organization it belongs to.
    #[arg(long)]
    pub identifier: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    /// Join this hub's network (a name or a path), so the engine and the plugins it
    /// starts reach the hub from inside Docker.
    #[arg(long)]
    pub hub: Option<String>,
    /// Put the engine on the mesh, so the plugins it starts reach the hub it is bound to
    /// over the tailnet — from this machine or any other. A mesh key is asked for with
    /// the authorization; the engine is not written if none is granted.
    #[arg(long)]
    pub mesh: bool,
    /// Write it, but leave it stopped.
    #[arg(long)]
    pub no_start: bool,
    /// Do not open a browser for the authorization.
    #[arg(long)]
    pub no_open: bool,
}

pub async fn run(args: EngineCreateArgs) -> Result<()> {
    ui::say("");
    ui::say(&format!("  {}", ui::bold("Creating a plugin engine")));
    ui::say("");

    let requested = args.dir.as_deref().unwrap_or(".");
    std::fs::create_dir_all(requested).with_context(|| format!("creating {requested}"))?;
    let dir = konstruktor_core::paths::canonical(requested).with_context(|| format!("resolving {requested}"))?;

    // The shared discriminator, not a raw file check — so this names what is actually
    // there rather than calling a hub or a coordination server "a compose project".
    if let Some(kind) = konstruktor_core::profile::holds_a_deployment(&dir) {
        bail!(
            "{} already holds a {}. Create the engine in an empty folder.",
            dir.display(),
            kind.label()
        );
    }

    let name = args
        .name
        .clone()
        .unwrap_or_else(|| konstruktor_core::compose::basename(&dir.to_string_lossy()));

    let server = args
        .server
        .clone()
        .unwrap_or_else(|| konstruktor_core::defaults::COORDINATION_SERVER.to_string());

    let identifier = args
        .identifier
        .clone()
        .unwrap_or_else(|| identifier_from_folder(&dir));

    let hub = args.hub.as_deref().map(resolve_hub).transpose()?;

    let answers = EngineAnswers {
        dir: dir.to_string_lossy().to_string(),
        name,
        coord_server: server,
        identifier,
        description: args.description.clone(),
        start: !args.no_start,
        hub: hub.as_ref().map(|h| h.to_string_lossy().to_string()),
        mesh: args.mesh,
    };

    let mut rows = vec![
        ("folder".into(), answers.dir.clone()),
        ("coordination".into(), answers.coord_server.clone()),
        ("identifier".into(), answers.identifier.clone()),
        ("runs".into(), DEPLOYER_IMAGE.to_string()),
    ];
    if let Some(hub) = &answers.hub {
        rows.push(("attached to".into(), hub.clone()));
    }
    if answers.mesh {
        rows.push((
            "mesh".into(),
            "joins as the app it is authorized as; plugins run in its namespace".into(),
        ));
    }
    ui::table(&rows);
    ui::say("");

    // Ctrl-C during the wait cancels the poll rather than leaving it running.
    let cancel = CancellationToken::new();
    let on_signal = cancel.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        on_signal.cancel();
    });

    let open_browser = !args.no_open && ui::is_interactive();
    let created = create_engine(&answers, &cancel, &|event| {
        crate::create::report(event, open_browser)
    })
    .await?;

    ui::say("");
    ui::ok(&format!(
        "The engine is at {}.",
        ui::bold(&created.path.to_string_lossy())
    ));
    ui::say("");
    Ok(())
}

#[derive(Args, Debug, Clone)]
pub struct EngineAttachArgs {
    /// The engine: a path, or the name of a registered one. Defaults to here.
    pub engine: Option<String>,
    /// The hub whose network it joins: a name or a path.
    #[arg(long)]
    pub hub: String,
}

#[derive(Args, Debug, Clone)]
pub struct EngineDetachArgs {
    /// The engine: a path, or the name of a registered one. Defaults to here.
    pub engine: Option<String>,
}

/// A hub's folder, from its name or path — refused, by name, when it is not a hub.
fn resolve_hub(given: &str) -> Result<std::path::PathBuf> {
    crate::manage::Target {
        target: Some(given.to_string()),
    }
    .resolve()
}

fn resolve_engine(given: Option<String>) -> Result<std::path::PathBuf> {
    let resolved = crate::manage::Target { target: given }.resolve_any()?;
    if resolved.kind != konstruktor_core::profile::DeploymentKind::Engine {
        bail!(
            "{} is a {}, not a plugin engine",
            resolved.dir.display(),
            resolved.kind.label()
        );
    }
    Ok(resolved.dir)
}

/// `konstruktor engine attach` / `detach`: rewrites the engine's compose file, then
/// restarts the engine if it is running so the deployer picks the network up.
pub async fn attach(engine: Option<String>, hub: Option<String>) -> Result<()> {
    let dir = resolve_engine(engine)?;
    let hub = hub.as_deref().map(resolve_hub).transpose()?;

    konstruktor_core::engine::attach(&dir, hub.as_deref()).await?;
    ui::say("");
    match &hub {
        Some(hub) => ui::ok(&format!(
            "The engine now joins the network of {}.",
            ui::bold(&hub.to_string_lossy())
        )),
        None => ui::ok("The engine is detached; plugins run on its own network."),
    }

    let containers = konstruktor_core::docker::list_deployment_containers(&dir.to_string_lossy())
        .await
        .unwrap_or_default();
    if konstruktor_core::status::run_summary(&containers).state
        == konstruktor_core::status::RunState::Stopped
        || containers.is_empty()
    {
        ui::step("Start the engine to apply it.");
        ui::say("");
        return Ok(());
    }

    ui::step("Restarting the engine to apply it…");
    let print = |line: konstruktor_core::compose::ComposeLine| {
        eprintln!("  {}", ui::dim(&line.line));
    };
    let report = konstruktor_core::start::start(&dir, &print).await?;
    for warning in &report.warnings {
        ui::warn(warning);
    }
    ui::ok("Done. Plugins already running stay where they are until they are restarted.");
    ui::say("");
    Ok(())
}
