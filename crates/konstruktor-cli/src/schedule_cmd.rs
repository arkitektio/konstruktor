//! `konstruktor schedule`: the jobs a hub runs by itself.
//!
//! Which jobs would like to run on their own is the images' to say; whether they do, and
//! when, is the operator's, kept in the profile. `tick` is what runs them — whatever wakes
//! the hub calls it — and does what is due, one job after the other.

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Result};
use clap::{Args, Subcommand};
use konstruktor_core::schedule::{self, Schedule, ScheduleChoice};
use konstruktor_core::{contract, profile};

use crate::manage::Target;
use crate::ui;

#[derive(Subcommand, Debug, Clone)]
pub enum ScheduleCommand {
    /// List the jobs that can run by themselves: when, whether, and how the last run went.
    List(InArgs),
    /// Let a job run by itself, on its image's schedule or the one given.
    On(ChangeArgs),
    /// Stop a job from running by itself. It can still be run with `job run`.
    Off(ChangeArgs),
    /// Run every job that is due now, one after the other.
    ///
    /// What wakes the hub calls this, every minute or so; it does nothing for a job that is
    /// not due, already ran in its window, or while an update runs.
    Tick(TickArgs),
}

#[derive(Args, Debug, Clone)]
pub struct InArgs {
    /// The deployment: a path, or a name from `konstruktor list`. Defaults to here.
    #[arg(long = "in", value_name = "TARGET")]
    pub in_deployment: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct ChangeArgs {
    /// The service, as `konstruktor ps` names it.
    pub service: String,
    /// The job, as `konstruktor schedule list` names it.
    pub job: String,
    /// When instead: `hourly`, `daily HH:MM[-HH:MM]` or `weekly <mon..sun> HH:MM[-HH:MM]`,
    /// in this machine's local time. The part after `-` is when it must be done.
    #[arg(long, value_name = "SCHEDULE")]
    pub at: Option<String>,
    #[command(flatten)]
    pub target: InArgs,
}

#[derive(Args, Debug, Clone)]
pub struct TickArgs {
    /// Say what would run, and run nothing.
    #[arg(long)]
    pub dry_run: bool,
    #[command(flatten)]
    pub target: InArgs,
}

pub async fn run(command: ScheduleCommand, json: bool) -> Result<()> {
    match command {
        ScheduleCommand::List(args) => list(args, json).await,
        ScheduleCommand::On(args) => change(args, true).await,
        ScheduleCommand::Off(args) => change(args, false).await,
        ScheduleCommand::Tick(args) => tick(args, json).await,
    }
}

fn when(time: chrono::DateTime<chrono::Utc>) -> String {
    time.with_timezone(&chrono::Local)
        .format("%a %d %b %H:%M")
        .to_string()
}

async fn list(args: InArgs, json: bool) -> Result<()> {
    let dir = Target::named(args.in_deployment).resolve()?;
    let config = profile::read_profile(&dir)?.config;
    let described = contract::described(&dir, &config).await;
    let planned = schedule::planned(&config, &described);
    let state = schedule::read_state(&dir);
    let running = schedule::running_jobs(&dir);

    if json {
        let rows: Vec<_> = planned
            .iter()
            .map(|plan| {
                let id = format!("{}/{}", plan.service, plan.job);
                serde_json::json!({
                    "plan": plan,
                    "running": running.contains(&id),
                    "last": state.runs.get(&id),
                })
            })
            .collect();
        return ui::emit_json(&rows);
    }

    ui::say("");
    if planned.is_empty() {
        ui::say(&format!(
            "  {}",
            ui::dim("No service of this hub offers a job to run by itself.")
        ));
        ui::say("");
        return Ok(());
    }
    let now = chrono::Local::now();
    let rows: Vec<(String, String)> = planned
        .iter()
        .map(|plan| {
            let id = format!("{}/{}", plan.service, plan.job);
            let mut about = format!(
                "{} {}",
                if plan.enabled { "on " } else { "off" },
                plan.schedule.as_deref().unwrap_or_default()
            );
            if plan.schedule != plan.suggested {
                if let Some(suggested) = &plan.suggested {
                    about.push_str(&ui::dim(&format!(" (suggested: {suggested})")));
                }
            }
            if plan.long_running {
                about.push_str(&ui::dim(" · long-running"));
            }
            if running.contains(&id) {
                about.push_str(" · running now");
            } else if let Some(run) = state.runs.get(&id) {
                about.push_str(&format!(
                    " · last {} {}",
                    when(run.started),
                    match run.outcome {
                        schedule::Outcome::Succeeded => "ok".to_string(),
                        schedule::Outcome::Failed => "failed".to_string(),
                        schedule::Outcome::NothingOwed => "nothing owed".to_string(),
                        schedule::Outcome::Running => "did not finish".to_string(),
                    }
                ));
            }
            if plan.enabled {
                if let Some(Ok(parsed)) = plan.schedule.as_deref().map(str::parse::<Schedule>) {
                    about.push_str(&ui::dim(&format!(
                        " · next {}",
                        when(parsed.next_after(&now).with_timezone(&chrono::Utc))
                    )));
                }
            }
            (id, about)
        })
        .collect();
    ui::table(&rows);
    ui::say("");
    ui::say(&ui::dim(
        "  Jobs run when `konstruktor schedule tick` is called: from a timer, or by hand.",
    ));
    ui::say("");
    Ok(())
}

async fn change(args: ChangeArgs, enabled: bool) -> Result<()> {
    let dir = Target::named(args.target.in_deployment).resolve()?;
    let mut held = profile::read_profile(&dir)?;
    let description = contract::description_of(&dir, &held.config, &args.service)
        .await
        .ok_or_else(|| {
            anyhow!(
                "`{}` is not a service of this hub whose image describes itself",
                args.service
            )
        })?;
    let Some(job) = description.jobs.get(&args.job) else {
        bail!("`{}` offers no job `{}`", args.service, args.job);
    };
    if let Some(at) = &args.at {
        at.parse::<Schedule>().map_err(|error| anyhow!("{error}"))?;
    } else if enabled && job.schedule.is_none() {
        let previous = held
            .config
            .schedules
            .get(&args.service)
            .and_then(|jobs| jobs.get(&args.job))
            .and_then(|choice| choice.at.clone());
        if previous.is_none() {
            bail!(
                "`{}` suggests no schedule for `{}`: say when with `--at`, e.g. `--at \"daily 03:00\"`",
                args.service,
                args.job
            );
        }
    }
    let jobs: &mut BTreeMap<String, ScheduleChoice> = held
        .config
        .schedules
        .entry(args.service.clone())
        .or_default();
    let choice = jobs.entry(args.job.clone()).or_default();
    choice.enabled = enabled;
    if args.at.is_some() {
        choice.at = args.at.clone();
    }
    // A choice that says only what the image already says is not kept.
    if choice.enabled == job.schedule.is_some() && choice.at.is_none() {
        jobs.remove(&args.job);
    }
    if jobs.is_empty() {
        held.config.schedules.remove(&args.service);
    }
    profile::write_profile(&dir, &held)?;
    let schedule = args.at.or_else(|| job.schedule.clone()).unwrap_or_default();
    if enabled {
        ui::ok(&format!(
            "{} in {} runs by itself: {schedule}",
            args.job, args.service
        ));
        if job.long_running {
            ui::warn("It may take hours; the hub serves meanwhile, and a window's end stops it if it takes a deadline.");
        }
    } else {
        ui::ok(&format!(
            "{} in {} no longer runs by itself",
            args.job, args.service
        ));
    }
    Ok(())
}

async fn tick(args: TickArgs, json: bool) -> Result<()> {
    let dir = Target::named(args.target.in_deployment).resolve()?;
    let print = |line: konstruktor_core::compose::ComposeLine| {
        if !json {
            ui::say(&format!("  {}", line.line));
        }
    };
    let done = schedule::tick(&dir, args.dry_run, &print).await?;
    if json {
        return ui::emit_json(&done);
    }
    for ticked in &done {
        let id = format!("{}/{}", ticked.service, ticked.job);
        match ticked.what.as_str() {
            "ran" => ui::ok(&format!("{id} ran")),
            "nothing_owed" => ui::ok(&format!("{id} owed nothing")),
            "failed" => ui::fail(&format!(
                "{id} failed: {}",
                ticked.detail.as_deref().unwrap_or_default().trim()
            )),
            "due" => ui::say(&format!(
                "  {id}: {}",
                ticked.detail.as_deref().unwrap_or("due")
            )),
            _ => {}
        }
    }
    Ok(())
}
