//! The jobs a hub runs by itself.
//!
//! A service's image says which of its jobs it would like run on a schedule
//! ([`crate::contract::Job::schedule`]); the operator turns each on or off and may move it, in
//! the profile ([`ScheduleChoice`]). Nothing here keeps a timer: whatever wakes the hub —
//! a scheduler container, a host timer, a person — calls [`tick`], which runs what is due and
//! records it. A job is run exactly as `konstruktor job run` runs it, in a container of its
//! service's image, and both take the same lock, so a scheduled run and one asked for by hand
//! never overlap.
//!
//! **The grammar** is arkitekt-service's (`arkitekt_service.contract.schedule`), read the
//! same way: `hourly`, `daily HH:MM[-HH:MM]`, `weekly <mon..sun> HH:MM[-HH:MM]`, in the hub's
//! local time. The part after `-` is the window: a hub that was off at the start still runs
//! the job while the window is open, a job that takes a deadline is told its end, and
//! without an end a window is one hour.
//!
//! **One attempt per window.** A run that failed is not retried until the next window:
//! a job that fails at 01:00 would otherwise be started again every minute until 06:00.
//!
//! **Nothing runs while an update does.** An update stops a service to migrate it; a job
//! started then would run against a database halfway between two releases. [`Updating`] is
//! written when an update starts and removed when it ends; one older than
//! [`UPDATE_STALE_AFTER`] is taken for a crashed update and ignored, said so in the record.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use chrono::{DateTime, Datelike, Duration, NaiveTime, TimeZone, Timelike, Utc, Weekday};
use serde::{Deserialize, Serialize};

use crate::config::hub::HubConfig;
use crate::contract::{self, Job};

/// Where a hub keeps what its jobs did, beside its lock file.
pub const STATE_FILENAME: &str = "hub_schedule.json";
/// Where a hub's job locks are: one file per job, held while it runs.
pub const LOCKS_DIRNAME: &str = "hub_job_locks";
/// The mark an update leaves while it runs.
pub const UPDATING_FILENAME: &str = "hub_updating.json";
/// An update mark older than this was left by an update that never finished.
pub const UPDATE_STALE_AFTER: Duration = Duration::hours(6);
/// How many lines of a run's output are kept with its record.
const TAIL_LINES: usize = 20;

// --- the grammar ---------------------------------------------------------------------------

/// When a job is due, as the grammar says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Every {
    Hourly,
    Daily { at: NaiveTime },
    Weekly { day: Weekday, at: NaiveTime },
}

/// A schedule: when a run starts, and when its window closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    pub every: Every,
    /// The window's end, as a time of day; `None` is one hour after the start.
    pub until: Option<NaiveTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "`{0}` is not a schedule: say `hourly`, `daily HH:MM`, `daily HH:MM-HH:MM` or `weekly <mon|tue|wed|thu|fri|sat|sun> HH:MM[-HH:MM]`, in the hub's local time"
)]
pub struct NotASchedule(pub String);

const DAYS: [(&str, Weekday); 7] = [
    ("mon", Weekday::Mon),
    ("tue", Weekday::Tue),
    ("wed", Weekday::Wed),
    ("thu", Weekday::Thu),
    ("fri", Weekday::Fri),
    ("sat", Weekday::Sat),
    ("sun", Weekday::Sun),
];

/// `HH:MM`, two digits each, as the grammar spells it.
fn time_of_day(text: &str) -> Option<NaiveTime> {
    let (hours, minutes) = text.split_once(':')?;
    if hours.len() != 2 || minutes.len() != 2 {
        return None;
    }
    NaiveTime::from_hms_opt(hours.parse().ok()?, minutes.parse().ok()?, 0)
}

/// `HH:MM` or `HH:MM-HH:MM`.
fn start_and_end(text: &str) -> Option<(NaiveTime, Option<NaiveTime>)> {
    match text.split_once('-') {
        Some((start, end)) => Some((time_of_day(start)?, Some(time_of_day(end)?))),
        None => Some((time_of_day(text)?, None)),
    }
}

impl FromStr for Schedule {
    type Err = NotASchedule;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let refused = || NotASchedule(text.to_string());
        let words: Vec<&str> = text.split(' ').collect();
        match words.as_slice() {
            ["hourly"] => Ok(Schedule {
                every: Every::Hourly,
                until: None,
            }),
            ["daily", times] => {
                let (at, until) = start_and_end(times).ok_or_else(refused)?;
                Ok(Schedule {
                    every: Every::Daily { at },
                    until,
                })
            }
            ["weekly", day, times] => {
                let day = DAYS
                    .iter()
                    .find(|(name, _)| name == day)
                    .map(|(_, day)| *day)
                    .ok_or_else(refused)?;
                let (at, until) = start_and_end(times).ok_or_else(refused)?;
                Ok(Schedule {
                    every: Every::Weekly { day, at },
                    until,
                })
            }
            _ => Err(refused()),
        }
    }
}

/// A local date and time in `tz`, resolved through a DST change: the earlier of an
/// ambiguous hour, and the moment just after a skipped one.
fn at_local<Tz: TimeZone>(tz: &Tz, date: chrono::NaiveDate, time: NaiveTime) -> DateTime<Tz> {
    let naive = date.and_time(time);
    tz.from_local_datetime(&naive)
        .earliest()
        .or_else(|| {
            tz.from_local_datetime(&(naive + Duration::hours(1)))
                .earliest()
        })
        .unwrap_or_else(|| tz.from_utc_datetime(&naive))
}

impl Schedule {
    /// The window that opened last at or before `now`: when it opened, and when it closes.
    pub fn window_at<Tz: TimeZone>(&self, now: &DateTime<Tz>) -> (DateTime<Tz>, DateTime<Tz>) {
        let tz = now.timezone();
        let local = now.naive_local();
        let start = match self.every {
            Every::Hourly => at_local(
                &tz,
                local.date(),
                NaiveTime::from_hms_opt(local.time().hour(), 0, 0).expect("an hour"),
            ),
            Every::Daily { at } => {
                let today = at_local(&tz, local.date(), at);
                if today <= *now {
                    today
                } else {
                    at_local(&tz, local.date() - Duration::days(1), at)
                }
            }
            Every::Weekly { day, at } => {
                let back =
                    (local.weekday().num_days_from_monday() + 7 - day.num_days_from_monday()) % 7;
                let this_week = at_local(&tz, local.date() - Duration::days(back.into()), at);
                if this_week <= *now {
                    this_week
                } else {
                    at_local(&tz, local.date() - Duration::days(i64::from(back) + 7), at)
                }
            }
        };
        let end = match (self.every, self.until) {
            (Every::Hourly, _) | (_, None) => start.clone() + Duration::hours(1),
            (_, Some(until)) => {
                let date = start.naive_local().date();
                let same_day = at_local(&tz, date, until);
                if same_day > start {
                    same_day
                } else {
                    at_local(&tz, date + Duration::days(1), until)
                }
            }
        };
        (start, end)
    }

    /// When the next window opens after `now`.
    pub fn next_after<Tz: TimeZone>(&self, now: &DateTime<Tz>) -> DateTime<Tz> {
        let (start, _) = self.window_at(now);
        let step = match self.every {
            Every::Hourly => Duration::hours(1),
            Every::Daily { .. } => Duration::days(1),
            Every::Weekly { .. } => Duration::days(7),
        };
        // From the middle of the following period, so that a DST change cannot land the
        // next start back on this one, nor past the one after.
        let (next, _) = self.window_at(&(start + step + step / 2));
        next
    }
}

// --- the decision --------------------------------------------------------------------------

/// What a job's schedule says at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision<Tz: TimeZone> {
    /// Its window is open and it has not been attempted in it: run it, done by `deadline`.
    Due { deadline: DateTime<Tz> },
    /// It was attempted in the open window, or none is open: next at `next`.
    Waiting { next: DateTime<Tz> },
    /// An update is running: nothing starts until it is done.
    Paused,
}

/// Whether a job is due at `now`, given when it was last attempted. Pure: every input is
/// passed, so the rules are tested without a clock or a hub.
pub fn decide<Tz: TimeZone>(
    schedule: &Schedule,
    now: &DateTime<Tz>,
    last_attempt: Option<DateTime<Utc>>,
    updating: bool,
) -> Decision<Tz> {
    if updating {
        return Decision::Paused;
    }
    let (start, end) = schedule.window_at(now);
    let open = *now < end;
    let attempted = last_attempt.is_some_and(|last| last >= start.with_timezone(&Utc));
    if open && !attempted {
        Decision::Due { deadline: end }
    } else {
        Decision::Waiting {
            next: schedule.next_after(now),
        }
    }
}

// --- the operator's choice -----------------------------------------------------------------

/// The scheduling of one job, as the operator set it in the profile. A job the profile says
/// nothing of runs on its image's schedule.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleChoice {
    /// Whether it runs by itself at all.
    pub enabled: bool,
    /// When, instead of what the image suggests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

/// One job's schedule as the hub runs it: the operator's, over the image's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Planned {
    pub service: String,
    pub job: String,
    /// What the image suggests, if anything.
    pub suggested: Option<String>,
    /// What runs: the operator's, else the suggestion.
    pub schedule: Option<String>,
    pub enabled: bool,
    pub long_running: bool,
}

/// Every job of `described` that has a schedule, suggested or set, with whether it runs.
pub fn planned(
    config: &HubConfig,
    described: &BTreeMap<String, contract::Description>,
) -> Vec<Planned> {
    let mut out = Vec::new();
    for (service, description) in described {
        for (name, job) in &description.jobs {
            let choice = config
                .schedules
                .get(service)
                .and_then(|jobs| jobs.get(name));
            let schedule = choice
                .and_then(|choice| choice.at.clone())
                .or_else(|| job.schedule.clone());
            if schedule.is_none() {
                continue;
            }
            out.push(Planned {
                service: service.clone(),
                job: name.clone(),
                suggested: job.schedule.clone(),
                enabled: choice.map_or(job.schedule.is_some(), |choice| choice.enabled),
                schedule,
                long_running: job.long_running,
            });
        }
    }
    out
}

// --- what the jobs did ---------------------------------------------------------------------

/// Why a run happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Schedule,
    Manual,
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Still running, or killed before it could say.
    Running,
    Succeeded,
    Failed,
    /// Its backlog owed nothing, so it was not started.
    NothingOwed,
}

/// One run of a job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub trigger: Trigger,
    pub started: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<DateTime<Utc>>,
    pub outcome: Outcome,
    /// What its backlog said was owed before it started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owed: Option<u64>,
    /// The last lines it printed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tail: Vec<String>,
}

/// What a hub's jobs did, by `service/job`: the last run, and the last scheduled attempt.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub runs: BTreeMap<String, Run>,
    #[serde(default)]
    pub attempted: BTreeMap<String, DateTime<Utc>>,
}

fn key(service: &str, job: &str) -> String {
    format!("{service}/{job}")
}

fn state_path(dir: &Path) -> PathBuf {
    dir.join(STATE_FILENAME)
}

/// What the hub's jobs did, or nothing yet.
pub fn read_state(dir: &Path) -> State {
    std::fs::read_to_string(state_path(dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Read, change and write the state under one lock: a scheduled run and a manual one may
/// finish at the same moment.
fn change_state(dir: &Path, change: impl FnOnce(&mut State)) -> std::io::Result<()> {
    let guard = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(format!("{STATE_FILENAME}.lock")))?;
    guard.lock()?;
    let mut state = read_state(dir);
    change(&mut state);
    let text = serde_json::to_string_pretty(&state).expect("a state always serializes");
    let path = state_path(dir);
    let fresh = path.with_extension("json.tmp");
    std::fs::write(&fresh, text)?;
    std::fs::rename(fresh, path)
}

// --- the lock and the update mark ----------------------------------------------------------

/// A job's lock, held for as long as this lives.
#[derive(Debug)]
pub struct JobLock {
    _file: File,
}

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("`{job}` is already running in `{service}`")]
    Busy { service: String, job: String },
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Take `service`'s `job` lock, or say that it is running.
pub fn lock_job(dir: &Path, service: &str, job: &str) -> Result<JobLock, LockError> {
    let locks = dir.join(LOCKS_DIRNAME);
    std::fs::create_dir_all(&locks)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(locks.join(format!("{service}.{job}.lock")))?;
    match file.try_lock() {
        Ok(()) => Ok(JobLock { _file: file }),
        Err(std::fs::TryLockError::WouldBlock) => Err(LockError::Busy {
            service: service.to_string(),
            job: job.to_string(),
        }),
        Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

/// The jobs running now, as `service/job`: the ones whose lock somebody holds.
pub fn running_jobs(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir.join(LOCKS_DIRNAME)) else {
        return Vec::new();
    };
    let mut running: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let (service, job) = name.strip_suffix(".lock")?.split_once('.')?;
            let file = OpenOptions::new().write(true).open(entry.path()).ok()?;
            matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock))
                .then(|| key(service, job))
        })
        .collect();
    running.sort();
    running
}

/// The mark an update leaves while it runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Updating {
    pub since: DateTime<Utc>,
}

/// Leaves the update mark for as long as it lives; an update holds one from its start.
#[derive(Debug)]
pub struct UpdateMark {
    path: PathBuf,
}

impl Drop for UpdateMark {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Mark the hub as updating until the returned mark is dropped.
pub fn mark_updating(dir: &Path) -> UpdateMark {
    let path = dir.join(UPDATING_FILENAME);
    let mark = Updating { since: Utc::now() };
    let _ = std::fs::write(
        &path,
        serde_json::to_string(&mark).expect("a mark always serializes"),
    );
    UpdateMark { path }
}

/// Whether an update is running now: a mark younger than [`UPDATE_STALE_AFTER`].
pub fn updating(dir: &Path, now: DateTime<Utc>) -> bool {
    std::fs::read_to_string(dir.join(UPDATING_FILENAME))
        .ok()
        .and_then(|text| serde_json::from_str::<Updating>(&text).ok())
        .is_some_and(|mark| now - mark.since < UPDATE_STALE_AFTER)
}

// --- running -------------------------------------------------------------------------------

/// What a job's backlog says it owes: the `items` count on its last line.
pub fn owed_items(output: &str) -> Option<u64> {
    let last = output.lines().rev().find(|line| !line.trim().is_empty())?;
    serde_json::from_str::<serde_json::Value>(last.trim())
        .ok()?
        .get("items")?
        .as_u64()
}

fn tail_of(output: &str) -> Vec<String> {
    let lines: Vec<&str> = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    lines[lines.len().saturating_sub(TAIL_LINES)..]
        .iter()
        .map(|line| (*line).to_string())
        .collect()
}

/// The command that asks a job's backlog, in a container of its service's image.
pub fn backlog_command(service: &str, backlog: &[String]) -> Vec<String> {
    ["compose", "run", "--rm", "--no-deps", "-T", service]
        .into_iter()
        .map(String::from)
        .chain(backlog.iter().cloned())
        .collect()
}

/// How a run went, as [`run_job`] says it.
#[derive(Debug, Clone)]
pub struct Ran {
    pub run: Run,
    /// The combined output, when the run failed.
    pub error: Option<String>,
}

/// How a run is asked for: why, by when, and with what after the job's own command.
#[derive(Debug, Clone, Copy)]
pub struct Asked<'a> {
    pub trigger: Trigger,
    /// Passed with the job's deadline option, when it takes one.
    pub deadline: Option<DateTime<Utc>>,
    pub extra: &'a [String],
}

/// Run `service`'s `job` under its lock, and record it: first its backlog when it has one
/// (a scheduled run that owes nothing is not started), then the job, with the deadline as
/// its deadline option when it takes one.
pub async fn run_job(
    dir: &Path,
    service: &str,
    name: &str,
    job: &Job,
    asked: Asked<'_>,
    on_line: &(dyn Fn(crate::compose::ComposeLine) + Send + Sync),
) -> Result<Ran, LockError> {
    let Asked {
        trigger,
        deadline,
        extra,
    } = asked;
    let _lock = lock_job(dir, service, name)?;
    let started = Utc::now();
    let id = key(service, name);
    let mut run = Run {
        trigger,
        started,
        finished: None,
        outcome: Outcome::Running,
        owed: None,
        tail: Vec::new(),
    };
    change_state(dir, |state| {
        if trigger == Trigger::Schedule {
            state.attempted.insert(id.clone(), started);
        }
        state.runs.insert(id.clone(), run.clone());
    })?;

    if trigger == Trigger::Schedule {
        if let Some(backlog) = job.backlog.as_deref() {
            match crate::compose::run_streamed(dir, backlog_command(service, backlog), on_line)
                .await
            {
                Ok(output) => {
                    run.owed = owed_items(&output);
                    if run.owed == Some(0) {
                        run.outcome = Outcome::NothingOwed;
                        run.finished = Some(Utc::now());
                        change_state(dir, |state| {
                            state.runs.insert(id.clone(), run.clone());
                        })?;
                        return Ok(Ran { run, error: None });
                    }
                }
                // A backlog that cannot answer is no reason not to do the work.
                Err(said) => on_line(crate::compose::ComposeLine {
                    line: format!(
                        "the backlog of `{name}` did not answer; running it anyway: {}",
                        said.trim()
                    ),
                    stderr: true,
                }),
            }
        }
    }

    let mut arguments: Vec<String> = Vec::new();
    if let (Some(option), Some(deadline)) = (job.deadline.as_deref(), deadline) {
        arguments.push(option.to_string());
        arguments.push(deadline.to_rfc3339());
    }
    arguments.extend(extra.iter().cloned());
    let result = crate::compose::run_streamed(
        dir,
        contract::job_command(service, job, &arguments),
        on_line,
    )
    .await;
    run.finished = Some(Utc::now());
    let error = match &result {
        Ok(output) => {
            run.outcome = Outcome::Succeeded;
            run.tail = tail_of(output);
            None
        }
        Err(said) => {
            run.outcome = Outcome::Failed;
            run.tail = tail_of(said);
            Some(said.clone())
        }
    };
    change_state(dir, |state| {
        state.runs.insert(id.clone(), run.clone());
    })?;
    Ok(Ran { run, error })
}

/// What one tick did for one job.
#[derive(Debug, Clone, Serialize)]
pub struct Ticked {
    pub service: String,
    pub job: String,
    /// `ran`, `nothing_owed`, `failed`, `busy`, `waiting`, `paused`, `off`, `unreadable`.
    pub what: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Run every job that is due now, one after the other, and say what was done with each.
/// With `dry_run` nothing is run: each says what it would do.
pub async fn tick(
    dir: &Path,
    dry_run: bool,
    on_line: &(dyn Fn(crate::compose::ComposeLine) + Send + Sync),
) -> Result<Vec<Ticked>, crate::profile::ProfileError> {
    let config = crate::profile::read_profile(dir)?.config;
    let described = contract::described(dir, &config).await;
    let now = chrono::Local::now();
    let paused = updating(dir, now.with_timezone(&Utc));
    let state = read_state(dir);
    let mut out = Vec::new();
    for plan in planned(&config, &described) {
        let said = |what: &str, next: Option<DateTime<Utc>>, detail: Option<String>| Ticked {
            service: plan.service.clone(),
            job: plan.job.clone(),
            what: what.to_string(),
            next,
            detail,
        };
        if !plan.enabled {
            out.push(said("off", None, None));
            continue;
        }
        let text = plan.schedule.clone().unwrap_or_default();
        let schedule = match text.parse::<Schedule>() {
            Ok(schedule) => schedule,
            Err(error) => {
                out.push(said("unreadable", None, Some(error.to_string())));
                continue;
            }
        };
        let last = state.attempted.get(&key(&plan.service, &plan.job)).copied();
        match decide(&schedule, &now, last, paused) {
            Decision::Paused => out.push(said("paused", None, Some("an update is running".into()))),
            Decision::Waiting { next } => {
                out.push(said("waiting", Some(next.with_timezone(&Utc)), None))
            }
            Decision::Due { deadline } => {
                if dry_run {
                    out.push(said(
                        "due",
                        None,
                        Some(format!("would run, done by {}", deadline.to_rfc3339())),
                    ));
                    continue;
                }
                let job = &described[&plan.service].jobs[&plan.job];
                match run_job(
                    dir,
                    &plan.service,
                    &plan.job,
                    job,
                    Asked {
                        trigger: Trigger::Schedule,
                        deadline: Some(deadline.with_timezone(&Utc)),
                        extra: &[],
                    },
                    on_line,
                )
                .await
                {
                    Ok(ran) => out.push(match ran.run.outcome {
                        Outcome::NothingOwed => said("nothing_owed", None, None),
                        Outcome::Failed => said("failed", None, ran.error),
                        _ => said(
                            "ran",
                            None,
                            ran.run.owed.map(|owed| format!("{owed} owed before")),
                        ),
                    }),
                    Err(LockError::Busy { .. }) => {
                        out.push(said("busy", None, Some("already running".into())))
                    }
                    Err(LockError::Io(error)) => {
                        out.push(said("failed", None, Some(error.to_string())))
                    }
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn at(text: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(text).unwrap()
    }

    fn schedule(text: &str) -> Schedule {
        text.parse().unwrap()
    }

    #[test]
    fn the_grammar_is_arkitekt_services() {
        for good in [
            "hourly",
            "daily 03:00",
            "daily 01:00-06:00",
            "weekly sun 02:00-05:00",
            "daily 23:30-01:00",
        ] {
            assert!(good.parse::<Schedule>().is_ok(), "{good}");
        }
        for bad in [
            "",
            "nightly",
            "daily 3:00",
            "daily 24:00",
            "daily 03:00-",
            "weekly sunday 02:00",
            "*/5 * * * *",
            "daily  03:00",
        ] {
            assert!(bad.parse::<Schedule>().is_err(), "{bad}");
        }
    }

    #[test]
    fn a_daily_window_opens_at_its_start_and_closes_at_its_end() {
        let s = schedule("daily 01:00-06:00");
        let (start, end) = s.window_at(&at("2026-11-26T03:30:00+01:00"));
        assert_eq!(start, at("2026-11-26T01:00:00+01:00"));
        assert_eq!(end, at("2026-11-26T06:00:00+01:00"));
        // Before today's start, the window is yesterday's (and closed).
        let (start, _) = s.window_at(&at("2026-11-26T00:30:00+01:00"));
        assert_eq!(start, at("2026-11-25T01:00:00+01:00"));
    }

    #[test]
    fn a_window_may_run_past_midnight() {
        let (start, end) =
            schedule("daily 23:30-01:00").window_at(&at("2026-11-26T00:15:00+00:00"));
        assert_eq!(start, at("2026-11-25T23:30:00+00:00"));
        assert_eq!(end, at("2026-11-26T01:00:00+00:00"));
    }

    #[test]
    fn a_window_without_an_end_is_an_hour() {
        let (_, end) = schedule("daily 03:00").window_at(&at("2026-11-26T03:10:00+00:00"));
        assert_eq!(end, at("2026-11-26T04:00:00+00:00"));
    }

    #[test]
    fn a_weekly_window_is_on_its_day() {
        // 2026-11-26 is a Thursday.
        let (start, end) =
            schedule("weekly sun 02:00-05:00").window_at(&at("2026-11-26T12:00:00+00:00"));
        assert_eq!(start, at("2026-11-22T02:00:00+00:00"));
        assert_eq!(end, at("2026-11-22T05:00:00+00:00"));
        let next = schedule("weekly sun 02:00-05:00").next_after(&at("2026-11-26T12:00:00+00:00"));
        assert_eq!(next, at("2026-11-29T02:00:00+00:00"));
    }

    #[test]
    fn a_job_is_due_once_per_window() {
        let s = schedule("daily 01:00-06:00");
        let now = at("2026-11-26T02:00:00+00:00");
        assert_eq!(
            decide(&s, &now, None, false),
            Decision::Due {
                deadline: at("2026-11-26T06:00:00+00:00")
            }
        );
        // Attempted yesterday: due again today.
        let yesterday = at("2026-11-25T01:00:05+00:00").with_timezone(&Utc);
        assert!(matches!(
            decide(&s, &now, Some(yesterday), false),
            Decision::Due { .. }
        ));
        // Attempted in this window, failed or not: not again until tomorrow.
        let earlier = at("2026-11-26T01:00:05+00:00").with_timezone(&Utc);
        assert_eq!(
            decide(&s, &now, Some(earlier), false),
            Decision::Waiting {
                next: at("2026-11-27T01:00:00+00:00")
            }
        );
    }

    #[test]
    fn a_hub_that_was_off_catches_up_only_inside_the_window() {
        let s = schedule("daily 01:00-06:00");
        assert!(matches!(
            decide(&s, &at("2026-11-26T05:59:00+00:00"), None, false),
            Decision::Due { .. }
        ));
        assert_eq!(
            decide(&s, &at("2026-11-26T07:00:00+00:00"), None, false),
            Decision::Waiting {
                next: at("2026-11-27T01:00:00+00:00")
            }
        );
    }

    #[test]
    fn nothing_is_due_while_an_update_runs() {
        assert_eq!(
            decide(
                &schedule("hourly"),
                &at("2026-11-26T02:00:00+00:00"),
                None,
                true
            ),
            Decision::Paused
        );
    }

    #[test]
    fn hourly_is_due_each_hour() {
        let s = schedule("hourly");
        let now = at("2026-11-26T02:40:00+00:00");
        assert_eq!(
            decide(&s, &now, None, false),
            Decision::Due {
                deadline: at("2026-11-26T03:00:00+00:00")
            }
        );
        let this_hour = at("2026-11-26T02:00:01+00:00").with_timezone(&Utc);
        assert_eq!(
            decide(&s, &now, Some(this_hour), false),
            Decision::Waiting {
                next: at("2026-11-26T03:00:00+00:00")
            }
        );
    }

    #[test]
    fn a_backlog_answers_on_its_last_line() {
        assert_eq!(
            owed_items("loading model\n{\"items\": 17, \"bytes\": 3765064971}\n"),
            Some(17)
        );
        assert_eq!(owed_items("{\"items\": 0, \"bytes\": 0}"), Some(0));
        assert_eq!(owed_items("no json here"), None);
        assert_eq!(owed_items(""), None);
    }

    #[test]
    fn an_update_mark_counts_until_it_is_stale() {
        let dir = std::env::temp_dir().join(format!("konstruktor-schedule-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let now = Utc::now();
        {
            let _mark = mark_updating(&dir);
            assert!(updating(&dir, now));
            assert!(!updating(
                &dir,
                now + UPDATE_STALE_AFTER + Duration::minutes(1)
            ));
        }
        assert!(!updating(&dir, now), "the mark goes with the update");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_job_runs_once_at_a_time() {
        let dir = std::env::temp_dir().join(format!("konstruktor-joblock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let held = lock_job(&dir, "mikro", "digest_bigfiles").unwrap();
        assert!(matches!(
            lock_job(&dir, "mikro", "digest_bigfiles"),
            Err(LockError::Busy { .. })
        ));
        assert_eq!(
            running_jobs(&dir),
            vec!["mikro/digest_bigfiles".to_string()]
        );
        assert!(lock_job(&dir, "mikro", "purge_orphaned_stores").is_ok());
        drop(held);
        assert!(running_jobs(&dir).is_empty());
        assert!(lock_job(&dir, "mikro", "digest_bigfiles").is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
