# Scheduled jobs

How a hub runs its services' maintenance by itself: freeing the bytes of deleted data,
hashing stored files for duplicate detection, and whatever else a service declares. Written
for whoever changes a service, Konstruktor, or the contract between them.

The pieces live in three places:

| Piece | Where |
| --- | --- |
| What a job says about itself | arkitekt-service `Job` (`schedule`, `long_running`, `backlog`, `deadline`); its rules in `docs/migrations-and-jobs.md`, "Scheduled jobs" |
| Deciding and running what is due | `crates/konstruktor-core/src/schedule.rs` |
| What wakes it | the `scheduler` container (`docker/scheduler.Dockerfile`) or a host timer (`schedule_host.rs`) |

## 1. The contract

A service declares a job as before and may add four optional fields:

```python
"digest_bigfiles": Job(
    ("digest_bigfiles",),
    "Hash the stored files whose digest is still owed, for duplicate detection",
    schedule="daily 01:00-06:00",
    long_running=True,
    backlog=("--backlog",),
    deadline="--until",
),
```

- **`schedule`** is a suggestion, in the grammar below. The operator turns it on or off and
  may move it; the image never decides alone.
- **`long_running`**: a run may take hours. Such a job is refused in `setup=` (a hub waits
  for its setup before it serves) and warned about before it is run by hand.
- **`backlog`**: arguments that make the job say what it owes instead of doing it, from the
  database alone and quickly. Its last line of output is JSON with an `items` count. A
  scheduled run that owes nothing is not started.
- **`deadline`**: the option the job takes a deadline with. A scheduled run is passed its
  window's end as an ISO 8601 time; the job starts no new work after it and exits 0 with
  the rest still owed.

All four are optional on both sides. Konstruktor ignores keys it does not know, and an image
that says none of them is run as before.

## 2. The grammar

One grammar, parsed identically by `arkitekt_service.contract.schedule` and
`konstruktor_core::schedule`:

```
hourly                       every hour, on the hour
daily HH:MM                  every day
daily HH:MM-HH:MM            every day, done by the end
weekly <mon..sun> HH:MM[-HH:MM]
```

- Times are the **hub's local time**.
- The part after `-` is the **window**. A window without an end lasts one hour, and a window
  may run past midnight (`daily 23:30-01:00`).
- Two digits each; anything else (`nightly`, cron syntax, `3:00`) is refused where it is
  written.

## 3. When a job runs

`decide(schedule, now, last_attempt, updating)` is a pure function:

1. **Nothing starts while an update runs.** An update marks the hub (`hub_updating.json`)
   for as long as it runs, so no job starts against a database between two releases. A mark
   older than six hours is a crashed update's and is ignored.
2. **Once per window.** A job is due when its window is open and it has not been attempted
   in that window. A failed run is not retried until the next window: a job that fails at
   01:00 would otherwise start every minute until 06:00.
3. **A hub that was off catches up only inside the window.** Off at 01:00 and back at
   05:30: the job runs. Back at 07:00: it waits for tomorrow.

`tick` runs every due job, one after the other:

1. Ask the backlog, if the job declares one. When `items` is 0, record "nothing owed" and stop.
2. Run the job as `compose run --rm --no-deps -T <service> <command> [<deadline> <end>]`.
3. Record the outcome.

## 4. One run at a time, and a record

- **Lock.** Every run, scheduled or `konstruktor job run`, holds its job's lock
  (`hub_job_locks/<service>.<job>.lock`, a file lock that dies with its process). Two runs of
  one job never overlap.
- **No update under a running job.** An update refuses to start while any job holds its
  lock: the job's container would keep writing to a database the update stops and
  migrates. It takes its mark *before* looking, so nothing starts between the look and the
  update.
- **Record.** `hub_schedule.json` keeps, per `service/job`:
  - the last run: trigger, start, end, outcome, the backlog it saw, its last 20 lines;
  - the last scheduled attempt.

  Writes go through a lock of their own, so a scheduled run and a manual run can finish at
  the same moment.
- **Konstruktor's files, not a table.** The lock and the record live in the hub's folder
  rather than in a database table. Every run passes through Konstruktor anyway, and a table
  would need a migration in every service and would leave Lok out.

## 5. What wakes it

Exactly one thing per hub, chosen in the profile:

```yaml
scheduler:
  mode: container   # container | host | off
```

`tick` is told who called it (`--waker container|host`) and does nothing unless that is the
profile's mode. A timer left behind by another choice therefore never runs a job twice. A
person running `konstruktor schedule tick` runs whatever is due, regardless of the mode.

### Container (the default for new hubs)

A `scheduler` service in the hub's stack, image `jhnnsrs/konstruktor-scheduler`: the release's
static CLI with the Docker CLI and its compose plugin. It runs `konstruktor schedule serve`,
which ticks at the start of every minute.

- **One per hub.** `serve` holds `hub_scheduler.lock`; a second scheduler is refused.
- **The folder at its own path.** The compose file's bind mounts are resolved against the
  folder compose runs in and then mounted by the host's engine. Any other path would mount
  nothing. The path is recorded in the profile when the mode is set (`scheduler.path`);
  move the hub and set the mode again.
- **As the folder's owner** (`scheduler.user`, with the socket's group added), so the lock
  and record files stay writable for `konstruktor job run` on the host.
- **The host's local time**, mounted from `/etc/localtime`: schedules are written in it.
- **The Docker socket.** Whoever can start containers through it is root on the machine,
  in effect. A socket proxy would narrow the API but not that: `compose run` needs to create
  containers, and a container can be created privileged with the host mounted. Konstruktor
  says this when the mode is set; host mode exists for whoever will not have the socket in
  the hub.

### Host

A timer of the machine's own, calling `konstruktor schedule tick --waker host --in <hub>`
each minute:

| OS | Timer | Files |
| --- | --- | --- |
| Linux | systemd user service and timer, `OnCalendar=*-*-* *:*:00` | `~/.config/systemd/user/konstruktor-schedule-<hub>.{service,timer}` |
| macOS | launchd agent, `StartInterval 60` | `~/Library/LaunchAgents/live.arkitekt.konstruktor-schedule-<hub>.plist` |
| Windows | Task Scheduler, every minute | task `Konstruktor\konstruktor-schedule-<hub>` |

A user timer runs while the user is logged in. On a Linux server, `loginctl enable-linger`
keeps it running after logout; Konstruktor says so when it installs it.

### Switching

`konstruktor schedule mode <container|host|off>` (or the dashboard's "Woken by" buttons)
does three things in order:

1. Takes away what woke the jobs before: `compose rm --stop --force scheduler`, or the
   timer's files and units.
2. Writes the profile and regenerates the hub's files.
3. Sets up the new waker: `compose up -d scheduler` if the hub is up, or installs the timer.

## 6. The operator's say

The profile keeps only choices that differ from what the image suggests:

```yaml
schedules:
  mikro:
    digest_bigfiles: { enabled: true, at: "daily 02:00-05:00" }
    purge_orphaned_stores: { enabled: false }
```

| Surface | What it offers |
| --- | --- |
| CLI | `konstruktor schedule list`, `on <service> <job> [--at …]`, `off …`, `mode …`, `tick [--dry-run]`; `job list` shows suggestions, `job run` takes the lock and warns before a long job |
| Desktop | The dashboard's "Scheduled jobs": what wakes them; per job its schedule, last and next run, an on/off toggle, and "Run now" (confirmed for a long job) |

## 7. What the services declare (2026-11)

| Service | Jobs suggested |
| --- | --- |
| mikro, elektro | `purge_orphaned_stores` daily 03:00; `digest_bigfiles` daily 01:00–06:00 |
| bank, kuvert | the same two |
| dokuments, lovekit, rekuest, kabinet | `purge_orphaned_stores` daily 03:00 |
| kraph | both declared without a schedule, until it moves to arkitekt-service 10 |

Until `purge_orphaned_stores` was declared, nothing ran it in most services: deleted data's
bytes and abandoned uploads were never freed.

## 8. Adding a scheduled job

1. Make the command re-runnable on a serving hub. It finds what is owed, does it, and does
   nothing when nothing is.
2. Declare it with a `schedule`. Add `backlog` when asking is much cheaper than doing, and
   `deadline` when it can stop part-way.
3. Add `prepared.backlogs_answer(contract)` to the service's `tests/test_prepared.py`.
4. Never schedule a job that deletes data on request (`--yes`): those stay a person's to run.
