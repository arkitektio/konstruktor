//! `konstruktor coord create`: the third creation path, next to `hub` and `engine`.
//!
//! A coordination server is what the other two authorize *against* — where users,
//! organizations and permissions live. `go.arkitekt.live` is one; this is for running your
//! own. It asks nothing about identifiers or coordination servers, because it *is* the
//! coordination server: there is nobody to introduce itself to.
//!
//! The generator behind it is not written yet — see [`konstruktor_core::coord`] for
//! exactly what is missing. Everything else about a coordination server works: the folder
//! is recognised as a deployment and every lifecycle command drives it like any other
//! compose project.

use anyhow::{bail, Context, Result};
use clap::Args;
use konstruktor_core::coord::{create_coord, CoordAnswers};

use crate::ui;

#[derive(Args, Debug, Clone)]
pub struct CoordCreateArgs {
    /// Where the coordination server lives. Defaults to the current directory.
    pub dir: Option<String>,
    /// How this deployment is labelled. Defaults to the folder's name.
    #[arg(long)]
    pub name: Option<String>,
    /// The address clients will reach it at.
    ///
    /// This is what hubs and engines pass to `--server`, so it has to be reachable from
    /// wherever they run — not just from this machine.
    #[arg(long)]
    pub domain: Option<String>,
    #[arg(long, default_value_t = 8080)]
    pub http_port: u16,
    #[arg(long, default_value_t = 8443)]
    pub https_port: u16,
    #[arg(long)]
    pub ssl: bool,
    /// The first account, which everything else is administered through.
    #[arg(long, default_value = "admin")]
    pub admin: String,
    /// Left out, a strong one is generated.
    #[arg(long)]
    pub admin_password: Option<String>,
    #[arg(long)]
    pub admin_email: Option<String>,
    /// Skip `docker compose up -d`.
    #[arg(long)]
    pub no_start: bool,
    /// Print what would be written and stop.
    #[arg(long)]
    pub dry_run: bool,
}

pub async fn run(args: CoordCreateArgs) -> Result<()> {
    ui::say("");
    ui::say(&format!("  {}", ui::bold("Creating a coordination server")));
    ui::say("");

    let requested = args.dir.as_deref().unwrap_or(".");
    std::fs::create_dir_all(requested).with_context(|| format!("creating {requested}"))?;
    let dir = std::fs::canonicalize(requested).with_context(|| format!("resolving {requested}"))?;

    if let Some(kind) = konstruktor_core::profile::holds_a_deployment(&dir) {
        bail!(
            "{} already holds a {}. Create the coordination server in an empty folder.",
            dir.display(),
            kind.label()
        );
    }

    let name = args
        .name
        .clone()
        .unwrap_or_else(|| konstruktor_core::compose::basename(&dir.to_string_lossy()));

    let answers = CoordAnswers {
        dir: dir.to_string_lossy().to_string(),
        name,
        domain: args.domain.clone(),
        http_port: args.http_port,
        https_port: args.https_port,
        ssl: args.ssl,
        admin: args.admin.clone(),
        admin_password: args.admin_password.clone(),
        admin_email: args.admin_email.clone(),
        start: !args.no_start,
    };

    ui::table(&[
        ("folder".into(), answers.dir.clone()),
        ("name".into(), answers.name.clone()),
        (
            "address".into(),
            answers.domain.clone().unwrap_or_else(|| "localhost".into()),
        ),
        (
            "ports".into(),
            format!("{} / {}", answers.http_port, answers.https_port),
        ),
        ("admin".into(), answers.admin.clone()),
    ]);
    ui::say("");

    if args.dry_run {
        // Honest about the fact that there is nothing to preview yet, rather than
        // printing an empty file list as though that were the answer.
        ui::step("Nothing to preview: the generator is not written yet.");
        ui::say("");
        return Ok(());
    }

    let created = create_coord(&answers).await?;

    ui::ok(&format!(
        "Coordination server created at {}",
        created.path.to_string_lossy()
    ));
    ui::say("");
    println!("{}", created.path.to_string_lossy());
    Ok(())
}
