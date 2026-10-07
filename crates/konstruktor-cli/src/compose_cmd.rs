//! `konstruktor compose …`: the hub's compose file, by hand — what the desktop app's
//! compose editor does, from a terminal.
//!
//! Everything goes through `konstruktor_core::compose_file`, so an edit made here keeps
//! the same `.bak` the editor keeps, and is judged by the same `compose config`.

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Subcommand};
use konstruktor_core::compose_file;

use crate::manage::Target;
use crate::ui;

#[derive(Subcommand, Debug, Clone)]
pub enum ComposeCommand {
    /// Print the compose file, what the generator would write, or the previous version.
    Show(ShowArgs),
    /// Ask the engine whether it accepts the file (`docker compose config`).
    Validate(Target),
    /// Open the file in $EDITOR, then check what you saved.
    ///
    /// The previous version is kept as docker-compose.yaml.bak.
    Edit(Target),
    /// Replace the file with the generated one, undoing every hand edit.
    ///
    /// The previous version is kept. Only the compose file: `hub regenerate` rewrites
    /// the gateway and service configs as well.
    Reset(ConfirmArgs),
    /// Put back the file as it was before the last edit or reset.
    // `restore-backup` is what this was called while "backup" still meant two things —
    // this file's `.bak`, and `konstruktor backup` of the data.
    #[command(alias = "restore-backup")]
    Undo(ConfirmArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ShowArgs {
    #[command(flatten)]
    pub target: Target,
    /// What the generator would write from the profile, instead of the file on disk.
    #[arg(long, conflicts_with = "previous")]
    pub generated: bool,
    /// The version before the last edit or reset.
    #[arg(long, alias = "backup")]
    pub previous: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ConfirmArgs {
    #[command(flatten)]
    pub target: Target,
    /// Answer yes to every confirmation. Required when this is not a terminal.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

pub async fn run(command: ComposeCommand) -> Result<()> {
    match command {
        ComposeCommand::Show(args) => {
            let dir = args.target.resolve()?;
            let text = if args.generated {
                compose_file::regenerate(&dir)?
            } else if args.previous {
                compose_file::read_backup(&dir)?.ok_or_else(|| {
                    anyhow!("there is no previous version — nothing has been edited yet")
                })?
            } else {
                compose_file::read(&dir)?
            };
            print!("{text}");
            Ok(())
        }
        ComposeCommand::Validate(target) => {
            let dir = target.resolve()?;
            validate(&dir).await
        }
        ComposeCommand::Edit(target) => {
            let dir = target.resolve()?;
            edit(&dir).await
        }
        ComposeCommand::Reset(args) => {
            let dir = args.target.resolve()?;
            confirm(
                args.yes,
                "Replace the compose file with the generated one? Hand edits are lost; the \
                 current file is kept as the previous version.",
            )?;
            compose_file::write(&dir, &compose_file::regenerate(&dir)?)?;
            ui::ok("The compose file is what the generator writes again.");
            validate(&dir).await
        }
        ComposeCommand::Undo(args) => {
            let dir = args.target.resolve()?;
            let backup = compose_file::read_backup(&dir)?
                .ok_or_else(|| anyhow!("there is no previous version to put back"))?;
            confirm(
                args.yes,
                "Put the previous version back? The current file becomes the previous one.",
            )?;
            compose_file::write(&dir, &backup)?;
            ui::ok("The previous version is the compose file again.");
            validate(&dir).await
        }
    }
}

/// `hub regenerate`: every generated file again, from the profile as it stands.
///
/// The hub keeps its keys, secrets and images. It undoes hand edits and picks up what the
/// generator writes differently for the layout the hub already has — and what the hub's
/// images have said they need that it does not hold yet, a database, a bucket or a secret,
/// which is created here. A hub whose files are of an older layout is refused and sent to
/// `update`, which moves its services with its files.
pub async fn regenerate_hub(args: ConfirmArgs) -> Result<()> {
    let dir = args.target.resolve()?;
    let config = konstruktor_core::profile::read_profile(&dir)?.config;
    if let Some(reason) = konstruktor_core::migrate::behind(&dir, &config) {
        anyhow::bail!("{reason}");
    }
    confirm(
        args.yes,
        "Write this hub's generated files again (compose file, gateway, service configs)? \
         Hand edits to them are lost; the current compose file is kept as the previous \
         version.",
    )?;

    let provided = konstruktor_core::profile::regenerate(&dir)?;
    // As it was just written: it may hold what a service was newly provided.
    let config = konstruktor_core::profile::read_profile(&dir)?.config;
    if !provided.is_empty() {
        for (what, names) in [
            ("database", &provided.databases),
            ("bucket", &provided.buckets),
            ("secret", &provided.secrets),
        ] {
            if !names.is_empty() {
                ui::step(&format!("new {what}: {}", names.join(", ")));
            }
        }
        let print = |line: konstruktor_core::compose::ComposeLine| {
            eprintln!("  {}", ui::dim(&line.line));
        };
        konstruktor_core::services::provision(&dir, &config, &provided, &print).await?;
    }
    // The services' configs are their own images' to write.
    let identity = konstruktor_core::credentials::read_credentials(&dir)
        .map(|credentials| credentials.issued_identity())
        .unwrap_or_default();
    konstruktor_core::contract::render_hub(&dir, &config, &identity).await?;
    ui::ok("The hub's files are what this Konstruktor generates.");
    ui::say("Nothing running has changed yet: `konstruktor restart` makes the hub read them.");
    validate(&dir).await
}

async fn validate(dir: &std::path::Path) -> Result<()> {
    match compose_file::validate(dir).await {
        Ok(()) => {
            ui::ok("The engine accepts the compose file.");
            Ok(())
        }
        Err(message) => bail!("the engine does not accept the compose file:\n{message}"),
    }
}

async fn edit(dir: &std::path::Path) -> Result<()> {
    if !ui::is_interactive() {
        bail!("`compose edit` opens an editor, and this is not a terminal");
    }
    let before = compose_file::read(dir)?;

    // Edited as a copy, and written back through the core only if it changed: the core
    // keeps the backup and refuses what does not parse, which a direct save would skip.
    let scratch =
        std::env::temp_dir().join(format!("konstruktor-compose-{}.yaml", std::process::id()));
    std::fs::write(&scratch, &before).context("writing a copy to edit")?;

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| {
            if cfg!(windows) {
                "notepad".into()
            } else {
                "vi".into()
            }
        });
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or("vi");
    let status = std::process::Command::new(program)
        .args(parts)
        .arg(&scratch)
        .status()
        .with_context(|| format!("opening {editor}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&scratch);
        bail!("{editor} exited with {status}; the compose file is unchanged");
    }

    let after = std::fs::read_to_string(&scratch).context("reading the edited copy")?;
    let _ = std::fs::remove_file(&scratch);
    if after == before {
        ui::step("No changes.");
        return Ok(());
    }

    compose_file::write(dir, &after)?;
    ui::ok("Saved. The previous version is docker-compose.yaml.bak.");
    if let Err(error) = validate(dir).await {
        ui::step(&ui::dim(
            "`konstruktor compose undo` puts the previous version back.",
        ));
        return Err(error);
    }
    ui::step(&ui::dim("`konstruktor up` applies it."));
    Ok(())
}

fn confirm(yes: bool, question: &str) -> Result<()> {
    if yes {
        return Ok(());
    }
    if !ui::is_interactive() {
        bail!("pass --yes to confirm");
    }
    if !inquire::Confirm::new(question)
        .with_default(false)
        .prompt()?
    {
        bail!("left alone");
    }
    Ok(())
}
