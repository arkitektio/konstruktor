//! `konstruktor schedule`: the jobs a hub runs by itself.
//!
//! Which jobs would like to run on their own is the images' to say; whether they do, and
//! when, is the operator's, kept in the profile. `tick` is what runs them — whatever wakes
//! the hub calls it — and does what is due, one job after the other.

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};
use konstruktor_core::schedule;

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
    /// Choose what wakes the jobs: the `scheduler` container, the host's own timer, or nothing.
    Mode(ModeArgs),
    /// Wake the jobs each minute, for as long as this runs: what the `scheduler` container runs.
    #[command(hide = true)]
    Serve(InArgs),
}

#[derive(clap::ValueEnum, Debug, Clone, Copy)]
pub enum ModeChoice {
    /// A `scheduler` container in the hub's stack, up when the hub is (the default).
    Container,
    /// A timer of this machine's own (systemd, launchd, Task Scheduler).
    Host,
    /// Nothing: jobs run only when asked for with `job run`.
    Off,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy)]
pub enum WakerChoice {
    Container,
    Host,
}

#[derive(Args, Debug, Clone)]
pub struct ModeArgs {
    pub mode: ModeChoice,
    #[command(flatten)]
    pub target: InArgs,
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
    /// Who is waking the hub. A container or a host timer runs only the jobs it is chosen
    /// for (`schedule mode`); left out, this is a person, and runs what is due regardless.
    #[arg(long, value_name = "WHO")]
    pub waker: Option<WakerChoice>,
    #[command(flatten)]
    pub target: InArgs,
}

pub async fn run(command: ScheduleCommand, json: bool) -> Result<()> {
    match command {
        ScheduleCommand::List(args) => list(args, json).await,
        ScheduleCommand::On(args) => change(args, true).await,
        ScheduleCommand::Off(args) => change(args, false).await,
        ScheduleCommand::Tick(args) => tick(args, json).await,
        ScheduleCommand::Mode(args) => mode(args).await,
        ScheduleCommand::Serve(args) => serve(args).await,
    }
}

async fn mode(args: ModeArgs) -> Result<()> {
    use konstruktor_core::config::hub::SchedulerMode;

    let dir = Target::named(args.target.in_deployment).resolve()?;
    let mode = match args.mode {
        ModeChoice::Container => SchedulerMode::Container,
        ModeChoice::Host => SchedulerMode::Host,
        ModeChoice::Off => SchedulerMode::Off,
    };
    let exe = std::env::current_exe()?;
    let print = |line: konstruktor_core::compose::ComposeLine| ui::say(&format!("  {}", line.line));
    let changed = schedule::set_mode(&dir, mode, &exe, &print).await?;
    for said in &changed.said {
        ui::ok(said);
    }
    Ok(())
}

async fn serve(args: InArgs) -> Result<()> {
    let dir = Target::named(args.in_deployment).resolve()?;
    let print = |line: konstruktor_core::compose::ComposeLine| println!("{}", line.line);
    let report = |done: &[schedule::Ticked]| {
        for ticked in done {
            if matches!(
                ticked.what.as_str(),
                "ran" | "nothing_owed" | "failed" | "busy"
            ) {
                println!(
                    "{}/{}: {}{}",
                    ticked.service,
                    ticked.job,
                    ticked.what,
                    ticked
                        .detail
                        .as_deref()
                        .map(|detail| format!(" ({})", detail.trim()))
                        .unwrap_or_default()
                );
            }
        }
    };
    schedule::serve(&dir, &print, &report).await?;
    Ok(())
}

fn when(time: chrono::DateTime<chrono::Utc>) -> String {
    time.with_timezone(&chrono::Local)
        .format("%a %d %b %H:%M")
        .to_string()
}

async fn list(args: InArgs, json: bool) -> Result<()> {
    use konstruktor_core::config::hub::SchedulerMode;

    let dir = Target::named(args.in_deployment).resolve()?;
    let overview = schedule::overview(&dir).await?;
    if json {
        return ui::emit_json(&overview);
    }

    ui::say("");
    if overview.jobs.is_empty() {
        ui::say(&format!(
            "  {}",
            ui::dim("No service of this hub offers a job to run by itself.")
        ));
        ui::say("");
        return Ok(());
    }
    let rows: Vec<(String, String)> = overview
        .jobs
        .iter()
        .map(|view| {
            let plan = &view.plan;
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
            if view.running {
                about.push_str(" · running now");
            } else if let Some(run) = &view.last {
                about.push_str(&format!(
                    " · last {} {}",
                    when(run.started),
                    match run.outcome {
                        schedule::Outcome::Succeeded => "ok",
                        schedule::Outcome::Failed => "failed",
                        schedule::Outcome::NothingOwed => "nothing owed",
                        schedule::Outcome::Running => "did not finish",
                    }
                ));
            }
            if let Some(next) = view.next {
                about.push_str(&ui::dim(&format!(" · next {}", when(next))));
            }
            (format!("{}/{}", plan.service, plan.job), about)
        })
        .collect();
    ui::table(&rows);
    ui::say("");
    ui::say(&ui::dim(match overview.mode {
        SchedulerMode::Container => "  The `scheduler` container wakes them.",
        SchedulerMode::Host => "  This machine's timer wakes them.",
        SchedulerMode::Off => {
            "  Nothing wakes them yet: `konstruktor schedule mode container` (or `host`) does."
        }
    }));
    ui::say("");
    Ok(())
}

async fn change(args: ChangeArgs, enabled: bool) -> Result<()> {
    let dir = Target::named(args.target.in_deployment).resolve()?;
    let plan = schedule::choose(&dir, &args.service, &args.job, enabled, args.at.as_deref())
        .await
        .map_err(|error| anyhow!("{error}"))?;
    match plan {
        Some(plan) if plan.enabled => {
            ui::ok(&format!(
                "{} in {} runs by itself: {}",
                args.job,
                args.service,
                plan.schedule.as_deref().unwrap_or_default()
            ));
            if plan.long_running {
                ui::warn("It may take hours; the hub serves meanwhile, and a window's end stops it if it takes a deadline.");
            }
        }
        _ => ui::ok(&format!(
            "{} in {} no longer runs by itself",
            args.job, args.service
        )),
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
    let waker = match args.waker {
        None => schedule::Waker::Manual,
        Some(WakerChoice::Container) => schedule::Waker::Container,
        Some(WakerChoice::Host) => schedule::Waker::Host,
    };
    let done = schedule::tick(&dir, waker, args.dry_run, &print).await?;
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
