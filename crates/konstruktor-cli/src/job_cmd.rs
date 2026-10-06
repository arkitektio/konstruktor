//! `konstruktor job`: what a hub's services offer to be run beside their start.
//!
//! A service's container serves, and nothing else. Whatever else can be done in its image —
//! bring its database to a release, create its operator account again — is a job the image
//! declares by name (`konstruktor_core::contract::Job`), and this is where an operator asks
//! for one. Nothing here knows any of them: the list is the image's.

use anyhow::{anyhow, bail, Result};
use clap::{Args, Subcommand};

use konstruktor_core::{contract, profile};

use crate::manage::Target;
use crate::ui;

#[derive(Subcommand, Debug, Clone)]
pub enum JobCommand {
    /// List the jobs a service's image offers, and which of them prepares it.
    List(ListArgs),
    /// Run one of a service's jobs, in a container of its own.
    ///
    /// What follows `--` is passed on to the job:
    /// `konstruktor job run mikro ensureadmin -- --username ada`.
    Run(RunArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ListArgs {
    /// The service, as `konstruktor ps` names it. Left out: every service's jobs.
    pub service: Option<String>,
    /// The deployment: a path, or a name from `konstruktor list`. Defaults to here.
    #[arg(long = "in", value_name = "TARGET")]
    pub in_deployment: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct RunArgs {
    /// The service, as `konstruktor ps` names it.
    pub service: String,
    /// The job, as `konstruktor job list` names it.
    pub job: String,
    /// Passed on to the job.
    #[arg(last = true)]
    pub extra: Vec<String>,
    /// The deployment: a path, or a name from `konstruktor list`. Defaults to here.
    #[arg(long = "in", value_name = "TARGET")]
    pub in_deployment: Option<String>,
}

pub async fn run(command: JobCommand, json: bool) -> Result<()> {
    match command {
        JobCommand::List(args) => list(args, json).await,
        JobCommand::Run(args) => run_one(args).await,
    }
}

/// What can be asked for a job: the hub's services, and its own coordination server.
fn services_of(config: &konstruktor_core::config::hub::HubConfig) -> Vec<String> {
    let mut services: Vec<String> = config
        .enabled_services()
        .into_iter()
        .map(|id| config.service(id))
        .filter(|block| block.image.is_some())
        .map(|block| block.host.clone())
        .collect();
    if let Some(lok) = config.running_lok() {
        services.push(lok.host.clone());
    }
    services
}

async fn list(args: ListArgs, json: bool) -> Result<()> {
    let dir = Target::named(args.in_deployment).resolve()?;
    let config = profile::read_profile(&dir)?.config;
    let services = match args.service {
        Some(service) => vec![service],
        None => services_of(&config),
    };

    let mut said = std::collections::BTreeMap::new();
    for service in services {
        let description = contract::description_of(&dir, &config, &service)
            .await
            .ok_or_else(|| {
                anyhow!("`{service}` is not a service of this hub whose image describes itself")
            })?;
        said.insert(service, description);
    }
    if json {
        let jobs: std::collections::BTreeMap<_, _> = said
            .iter()
            .map(|(service, description)| {
                (
                    service,
                    serde_json::json!({"prepare": description.prepare, "jobs": description.jobs}),
                )
            })
            .collect();
        return ui::emit_json(&jobs);
    }
    for (service, description) in &said {
        ui::say("");
        ui::say(&format!("  {}", ui::bold(service)));
        if description.jobs.is_empty() {
            ui::say(&format!("    {}", ui::dim("offers no jobs")));
        }
        let rows: Vec<(String, String)> = description
            .jobs
            .iter()
            .map(|(name, job)| {
                let mut about = job.summary.clone();
                if description.prepare.as_deref() == Some(name) {
                    about.push_str(" (run before every start on a new build)");
                }
                if !job.includes.is_empty() {
                    about.push_str(&format!(" Runs: {}.", job.includes.join(", ")));
                }
                (name.clone(), about.trim().to_string())
            })
            .collect();
        ui::table(&rows);
    }
    ui::say("");
    Ok(())
}

async fn run_one(args: RunArgs) -> Result<()> {
    let dir = Target::named(args.in_deployment).resolve()?;
    let config = profile::read_profile(&dir)?.config;
    let description = contract::description_of(&dir, &config, &args.service)
        .await
        .ok_or_else(|| {
            anyhow!(
                "`{}` is not a service of this hub whose image describes itself",
                args.service
            )
        })?;
    let Some(job) = description.jobs.get(&args.job) else {
        let offered: Vec<&str> = description.jobs.keys().map(String::as_str).collect();
        bail!(
            "`{}` offers no job `{}` — it offers {}",
            args.service,
            args.job,
            if offered.is_empty() {
                "none".to_string()
            } else {
                offered.join(", ")
            }
        );
    };

    ui::say("");
    ui::step(&format!(
        "Running {} in {}…",
        ui::bold(&args.job),
        ui::bold(&args.service)
    ));
    let print = |line: konstruktor_core::compose::ComposeLine| ui::say(&format!("  {}", line.line));
    konstruktor_core::compose::run_streamed(
        &dir,
        contract::job_command(&args.service, job, &args.extra),
        &print,
    )
    .await
    .map_err(|said| anyhow!("`{}` failed in `{}`:\n{said}", args.job, args.service))?;
    ui::ok(&format!("{} ran in {}", args.job, args.service));
    Ok(())
}
