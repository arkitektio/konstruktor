use anyhow::{bail, Result};
use clap::Args;
use konstruktor_core::create::identifier_from_folder;

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
    /// A line about the engine, sent to the coordination server with the request.
    #[arg(long)]
    pub description: Option<String>,
    /// Join this hub's network (a name or a path), so the engine and the plugins it
    /// starts reach the hub from inside Docker.
    #[arg(long)]
    pub hub: Option<String>,
    /// Put the engine on the mesh, so the plugins it starts reach the hub it is bound to
    /// over the tailnet — from this machine or any other. A mesh key is asked for with
    /// the authorization; the engine is not written if none is granted. A switch, unlike
    /// `hub create --mesh`: the key always comes from the coordination server.
    #[arg(long)]
    pub mesh: bool,
    /// Write the files, but do not start the containers.
    #[arg(long)]
    pub no_start: bool,
    /// Do not open a browser for the authorization.
    #[arg(long)]
    pub no_open: bool,
}

pub async fn run(args: EngineCreateArgs) -> Result<()> {
    // The engine is an app like any other: its release says what it is, and that it needs
    // this machine's container engine — which asking for an engine is asking to grant it.
    let dir = args.dir.clone().unwrap_or_else(|| ".".into());
    let server = match (&args.server, &args.hub) {
        (Some(server), _) => Some(server.clone()),
        // Beside a hub: accepted at the coordination server the hub is connected to.
        (None, Some(_)) => None,
        (None, None) => Some(konstruktor_core::defaults::COORDINATION_SERVER.to_string()),
    };
    let instance_id = args.identifier.clone().or_else(|| {
        konstruktor_core::paths::canonical(&dir)
            .ok()
            .map(|dir| identifier_from_folder(&dir))
    });
    ui::say("");
    ui::step(&ui::dim(
        "A plugin engine starts the apps the organization installs as containers on this \
         machine, so it is granted this machine's container engine.",
    ));
    crate::apps::attach(
        crate::apps::AttachArgs {
            release: konstruktor_core::apps::PLUGIN_ENGINE_RELEASE.to_string(),
            dir: Some(dir),
            name: args.name.clone(),
            hub: args.hub.clone(),
            server,
            flavour: None,
            mesh: args.mesh,
            instance_id,
            allow: vec!["engine".into()],
            decline: Vec::new(),
            devices: Vec::new(),
            folders: Vec::new(),
            description: args.description.clone(),
            no_start: args.no_start,
            no_open: args.no_open,
            yes: false,
        },
        "Creating a plugin engine",
    )
    .await
}

#[derive(Args, Debug, Clone)]
pub struct EngineAttachArgs {
    #[command(flatten)]
    pub engine: crate::manage::Target,
    /// The hub whose network it joins: a name or a path.
    #[arg(long)]
    pub hub: String,
}

#[derive(Args, Debug, Clone)]
pub struct EngineDetachArgs {
    #[command(flatten)]
    pub engine: crate::manage::Target,
}

/// A hub's folder, from its name or path — refused, by name, when it is not a hub.
fn resolve_hub(given: &str) -> Result<std::path::PathBuf> {
    crate::manage::Target::named(Some(given.to_string())).resolve()
}

fn resolve_engine(engine: &crate::manage::Target) -> Result<std::path::PathBuf> {
    let resolved = engine.resolve_any()?;
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
pub async fn attach(engine: crate::manage::Target, hub: Option<String>) -> Result<()> {
    let dir = resolve_engine(&engine)?;
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
