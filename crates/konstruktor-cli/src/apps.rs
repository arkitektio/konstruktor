//! `konstruktor app attach`: an Arkitekt app beside a hub, from its release.
//!
//! The release says what the app is and what it must be granted on this machine; this shows
//! that in words, asks for what is not given on the command line, and hands the rest to
//! `konstruktor_core::apps::attach_app` — the same call the desktop app makes. Capabilities
//! that give root-level access are granted by name (`--allow`), or by answering for each one
//! in a terminal; `--yes` never grants them.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use clap::Args;
use konstruktor_core::apps::{self, AttachAnswers, CapabilityKind};
use tokio_util::sync::CancellationToken;

use crate::ui;

#[derive(Args, Debug, Clone)]
pub struct AttachArgs {
    /// The app's release: its registry repository, and a release of it —
    /// `ghcr.io/arkitektio/plugin_engine:2.1.0`. Without a release, the newest.
    pub release: String,
    /// Where the app's deployment lives. Defaults to the current directory.
    #[arg(long)]
    pub dir: Option<String>,
    /// How this deployment is labelled. Defaults to the folder's name.
    #[arg(long)]
    pub name: Option<String>,
    /// The hub beside which it runs, a name or a path: it joins the hub's network, and is
    /// accepted into one of the organizations the hub is connected to.
    #[arg(long)]
    pub hub: Option<String>,
    /// The coordination server to accept it at. Defaults to the hub's.
    #[arg(long)]
    pub server: Option<String>,
    /// The flavour to run. Defaults to the first that runs on this machine.
    #[arg(long)]
    pub flavour: Option<String>,
    /// Put the app on the mesh, with a key asked for when it is accepted.
    #[arg(long)]
    pub mesh: bool,
    /// Which installation this is. Defaults to the folder's name.
    #[arg(long)]
    pub instance_id: Option<String>,
    /// Grant a capability that gives root-level access, by name. Repeatable.
    #[arg(long = "allow", value_name = "CAPABILITY")]
    pub allow: Vec<String>,
    /// Leave an optional capability out, by name. Repeatable.
    #[arg(long = "decline", value_name = "CAPABILITY")]
    pub decline: Vec<String>,
    /// The device for a device capability: `--device camera=/dev/video2`. Repeatable.
    #[arg(long = "device", value_name = "NAME=DEVICE", value_parser = parse_pair)]
    pub devices: Vec<(String, String)>,
    /// The folder for a folder capability: `--folder data=/srv/data`. Repeatable.
    #[arg(long = "folder", value_name = "NAME=FOLDER", value_parser = parse_pair)]
    pub folders: Vec<(String, String)>,
    /// A line about this installation, shown to whoever accepts it.
    #[arg(long)]
    pub description: Option<String>,
    /// Write the files, but do not start the container.
    #[arg(long)]
    pub no_start: bool,
    /// Do not open a browser for the acceptance.
    #[arg(long)]
    pub no_open: bool,
    /// Ask nothing: what is not on the command line is not granted.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

fn parse_pair(value: &str) -> Result<(String, String), String> {
    match value.split_once('=') {
        Some((name, host)) if !name.trim().is_empty() && !host.trim().is_empty() => {
            Ok((name.trim().to_string(), host.trim().to_string()))
        }
        _ => Err(format!("`{value}` is not NAME=HOSTSIDE")),
    }
}

pub async fn run(args: AttachArgs) -> Result<()> {
    attach(args, "Attaching an app").await
}

/// Attaches the app `args` name, under `heading`.
pub(crate) async fn attach(args: AttachArgs, heading: &str) -> Result<()> {
    ui::say("");
    ui::say(&format!("  {}", ui::bold(heading)));
    ui::say("");

    let requested = args.dir.as_deref().unwrap_or(".");
    std::fs::create_dir_all(requested).with_context(|| format!("creating {requested}"))?;
    let dir = konstruktor_core::paths::canonical(requested)
        .with_context(|| format!("resolving {requested}"))?;
    if let Some(kind) = konstruktor_core::profile::holds_a_deployment(&dir) {
        bail!(
            "{} already holds a {}. Attach the app in an empty folder.",
            dir.display(),
            kind.label()
        );
    }
    let hub = args
        .hub
        .as_deref()
        .map(|given| crate::manage::Target::named(Some(given.to_string())).resolve())
        .transpose()?;

    ui::progress("Reading the release…");
    let (reference, release) = apps::release(&args.release).await?;
    let flavour = apps::choose(&release, args.flavour.as_deref(), &apps::host().await)?;
    ui::end_progress();

    let interactive = ui::is_interactive() && !args.yes;
    let mut allow = args.allow.clone();
    let mut devices: BTreeMap<String, String> = args.devices.iter().cloned().collect();
    let mut folders: BTreeMap<String, String> = args.folders.iter().cloned().collect();
    let mut decline = args.decline.clone();

    let mut rows = vec![
        (
            "app".into(),
            format!(
                "{} {}",
                release.manifest.identifier, release.manifest.version
            ),
        ),
        ("release".into(), reference.clone()),
        ("flavour".into(), flavour.name.clone()),
        ("folder".into(), dir.to_string_lossy().to_string()),
    ];
    if let Some(hub) = &hub {
        rows.push(("beside".into(), hub.to_string_lossy().to_string()));
    }
    ui::table(&rows);
    ui::say("");
    if flavour.capabilities.is_empty() {
        ui::step("It asks for nothing on this machine beyond a container of its own.");
    } else {
        ui::step("It asks to be given:");
    }
    for capability in &flavour.capabilities {
        let why = capability
            .description
            .as_deref()
            .map(|d| format!(" — {d}"))
            .unwrap_or_default();
        let marker = if capability.gives_root() {
            ui::bold("root")
        } else {
            ui::dim("    ")
        };
        ui::say(&format!(
            "    {marker}  {}: {}{why}{}",
            ui::bold(&capability.name),
            capability.sentence(),
            if capability.required {
                ""
            } else {
                " (optional)"
            }
        ));
    }
    ui::say("");

    // What a terminal can still be asked: the host side of a device or a folder, and each
    // root-level capability by name. Nothing here is granted by one blanket answer.
    if interactive {
        for capability in &flavour.capabilities {
            let name = &capability.name;
            if decline.contains(name) {
                continue;
            }
            match &capability.kind {
                CapabilityKind::Device { .. } if !devices.contains_key(name) => {
                    let answer = inquire::Text::new(&format!("Which device for `{name}`?"))
                        .with_help_message(&capability.sentence())
                        .prompt()?;
                    if answer.trim().is_empty() {
                        decline.push(name.clone());
                        continue;
                    }
                    devices.insert(name.clone(), answer.trim().to_string());
                }
                CapabilityKind::Folder { .. } if !folders.contains_key(name) => {
                    let answer = inquire::Text::new(&format!("Which folder for `{name}`?"))
                        .with_help_message(&capability.sentence())
                        .prompt()?;
                    if answer.trim().is_empty() {
                        decline.push(name.clone());
                        continue;
                    }
                    folders.insert(name.clone(), answer.trim().to_string());
                }
                _ => {}
            }
            if capability.gives_root() && !allow.contains(name) {
                let granted =
                    inquire::Confirm::new(&format!("Grant `{name}`: {}?", capability.sentence()))
                        .with_default(false)
                        .prompt()?;
                if granted {
                    allow.push(name.clone());
                } else {
                    decline.push(name.clone());
                }
            }
        }
    }

    let answers = AttachAnswers {
        dir: dir.to_string_lossy().to_string(),
        name: args
            .name
            .clone()
            .unwrap_or_else(|| konstruktor_core::compose::basename(&dir.to_string_lossy())),
        release: reference,
        flavour: Some(flavour.name.clone()),
        coord_server: args.server.clone(),
        hub: hub.as_ref().map(|h| h.to_string_lossy().to_string()),
        mesh: args.mesh,
        instance_id: args.instance_id.clone(),
        allow,
        decline,
        devices,
        folders,
        description: args.description.clone(),
        start: !args.no_start,
    };
    // Refused here, with nobody sent to a browser yet, when something required is missing.
    apps::grant(flavour, &answers)?;

    let cancel = CancellationToken::new();
    let on_signal = cancel.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        on_signal.cancel();
    });
    let open_browser = !args.no_open && ui::is_interactive();
    let attached = apps::attach_app(&answers, &cancel, &|event| {
        crate::create::report(event, open_browser)
    })
    .await?;

    ui::say("");
    ui::ok(&format!(
        "{} {} is at {}{}.",
        release.manifest.identifier,
        release.manifest.version,
        ui::bold(&attached.path.to_string_lossy()),
        attached
            .organization
            .as_deref()
            .map(|org| format!(", in organization {org}"))
            .unwrap_or_default()
    ));
    let names: Vec<&str> = attached
        .granted
        .iter()
        .map(|g| g.capability.name.as_str())
        .collect();
    if !names.is_empty() {
        ui::step(&format!("Granted: {}.", names.join(", ")));
    }
    ui::say("");
    Ok(())
}
