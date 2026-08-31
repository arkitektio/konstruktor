use anyhow::{bail, Context, Result};
use clap::Args;
use konstruktor_core::create::identifier_from_folder;
use konstruktor_core::engine::{create_engine, EngineAnswers, DEPLOYER_IMAGE};

use tokio_util::sync::CancellationToken;

use crate::ui;

const DEFAULT_COORDINATION_SERVER: &str = "go.arkitekt.live";

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
    let dir = std::fs::canonicalize(requested).with_context(|| format!("resolving {requested}"))?;

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
        .unwrap_or_else(|| DEFAULT_COORDINATION_SERVER.to_string());

    let identifier = args
        .identifier
        .clone()
        .unwrap_or_else(|| identifier_from_folder(&dir));

    let answers = EngineAnswers {
        dir: dir.to_string_lossy().to_string(),
        name,
        coord_server: server,
        identifier,
        description: args.description.clone(),
        start: !args.no_start,
    };

    ui::table(&[
        ("folder".into(), answers.dir.clone()),
        ("coordination".into(), answers.coord_server.clone()),
        ("identifier".into(), answers.identifier.clone()),
        ("runs".into(), DEPLOYER_IMAGE.to_string()),
    ]);
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
