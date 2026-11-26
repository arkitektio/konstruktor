import { CalendarClock, Loader2, Play } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import * as api from "../../api";
import type { ScheduledJob, ScheduleOverview, SchedulerMode } from "../../api";
import { useAlerter } from "../../alerter/alerter-context";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { SectionHeading } from "../../layout/PageHeader";
import { cn } from "../../utils";

/**
 * The jobs the hub runs by itself (`konstruktor_core::schedule`), and what wakes them.
 *
 * A service's image suggests when a job should run; the operator turns each on or off
 * here, and chooses the one thing that wakes them all: the `scheduler` container, this
 * machine's own timer, or nothing. A job can always be run now, under the same lock a
 * scheduled run takes, so the two never overlap. Hidden on a hub whose services suggest
 * no job at all.
 */

const MODES: { mode: SchedulerMode; label: string; hint: string }[] = [
  {
    mode: "container",
    label: "Container",
    hint: "A `scheduler` container in the stack, up when the hub is. It reaches the Docker engine through its socket, which is as much as root on this machine.",
  },
  {
    mode: "host",
    label: "This machine",
    hint: "A timer of this machine's own (systemd, launchd, Task Scheduler) calls the konstruktor command. Keeps the Docker socket out of the hub.",
  },
  { mode: "off", label: "Off", hint: "Nothing runs a job by itself; Run now still does." },
];

const when = (iso: string | null | undefined) =>
  iso
    ? new Date(iso).toLocaleString(undefined, {
        weekday: "short",
        day: "numeric",
        month: "short",
        hour: "2-digit",
        minute: "2-digit",
      })
    : null;

const OUTCOME: Record<string, { label: string; tone: string }> = {
  succeeded: { label: "ok", tone: "text-success" },
  failed: { label: "failed", tone: "text-destructive" },
  nothing_owed: { label: "nothing owed", tone: "text-muted-foreground" },
  running: { label: "did not finish", tone: "text-warning" },
};

const JobRow = ({
  view,
  path,
  onChanged,
}: {
  view: ScheduledJob;
  path: string;
  onChanged: () => void;
}) => {
  const { alert } = useAlerter();
  const [busy, setBusy] = useState<"toggle" | "run" | null>(null);
  const [confirming, setConfirming] = useState(false);
  const id = `${view.service}/${view.job}`;

  const toggle = async () => {
    setBusy("toggle");
    try {
      await api.scheduleChoose(path, view.service, view.job, !view.enabled);
      onChanged();
    } catch (error) {
      alert({
        error: `Could not change ${id}`,
        message: String(error),
        subtitle: "Its schedule was left as it was.",
      });
    } finally {
      setBusy(null);
    }
  };

  const runNow = async () => {
    setConfirming(false);
    setBusy("run");
    onChanged();
    try {
      const run = await api.scheduleRunNow(path, view.service, view.job);
      if (run.outcome === "failed") {
        alert({
          error: `${id} failed`,
          message: (run.tail ?? []).join("\n") || "It said nothing.",
          subtitle: "Run by hand; its scheduled runs are unaffected.",
        });
      }
    } catch (error) {
      alert({ error: `Could not run ${id}`, message: String(error), subtitle: "" });
    } finally {
      setBusy(null);
      onChanged();
    }
  };

  const outcome = view.last ? OUTCOME[view.last.outcome] : null;
  const running = view.running || busy === "run";

  return (
    <div className="flex flex-col gap-1 border-b py-2 last:border-b-0">
      <div className="flex items-center gap-3">
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="truncate font-mono text-sm">{id}</span>
          <span className="text-xs text-muted-foreground">
            {view.schedule}
            {view.schedule !== view.suggested && view.suggested && ` (suggested: ${view.suggested})`}
            {view.long_running && " · may take hours"}
          </span>
        </div>
        <div className="flex flex-col items-end text-xs">
          {running ? (
            <span className="flex items-center gap-1">
              <Loader2 className="size-3 animate-spin" /> running
            </span>
          ) : (
            view.last && (
              <span>
                last {when(view.last.started)}{" "}
                <span className={cn(outcome?.tone)}>{outcome?.label}</span>
              </span>
            )
          )}
          {view.next && <span className="text-muted-foreground">next {when(view.next)}</span>}
        </div>
        <Button
          size="sm"
          variant={view.enabled ? "secondary" : "outline"}
          disabled={busy !== null}
          onClick={toggle}
          title={view.enabled ? "Stop it running by itself" : "Let it run by itself"}
        >
          {busy === "toggle" ? <Loader2 className="size-3 animate-spin" /> : view.enabled ? "On" : "Off"}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          disabled={busy !== null || running}
          onClick={() => (view.long_running ? setConfirming(true) : runNow())}
          title="Run it now"
        >
          <Play className="size-3" />
        </Button>
      </div>
      {confirming && (
        <div className="flex items-center gap-2 rounded-md bg-muted px-3 py-2 text-xs">
          <span className="flex-1">
            {id} may take hours. The hub serves meanwhile, and stopping it loses nothing it
            finished.
          </span>
          <Button size="sm" variant="ghost" onClick={() => setConfirming(false)}>
            Cancel
          </Button>
          <Button size="sm" onClick={runNow}>
            Run now
          </Button>
        </div>
      )}
    </div>
  );
};

export const JobsSection = ({ path }: { path: string }) => {
  const { alert } = useAlerter();
  const [overview, setOverview] = useState<ScheduleOverview | null>(null);
  const [switching, setSwitching] = useState<SchedulerMode | null>(null);

  const refresh = useCallback(() => {
    api
      .scheduleOverview(path)
      .then(setOverview)
      .catch(() => setOverview(null));
  }, [path]);

  useEffect(() => {
    refresh();
    // A run's outcome and the next start move on their own.
    const timer = setInterval(refresh, 30_000);
    return () => clearInterval(timer);
  }, [refresh]);

  if (!overview || overview.jobs.length === 0) return null;

  const switchTo = async (mode: SchedulerMode) => {
    setSwitching(mode);
    try {
      await api.scheduleMode(path, mode);
      refresh();
    } catch (error) {
      alert({
        error: "Could not change what wakes the jobs",
        message: String(error),
        subtitle: "What woke them before may have been taken away; choose again.",
      });
    } finally {
      setSwitching(null);
    }
  };

  const current = MODES.find((m) => m.mode === overview.mode);

  return (
    <div>
      <SectionHeading hint="Maintenance the services would like run by themselves: freeing deleted data's storage, hashing files for duplicate detection. Each runs once in its window, in the hub's local time.">
        <span className="flex items-center gap-2">
          <CalendarClock className="size-4" /> Scheduled jobs
        </span>
      </SectionHeading>
      <div className="mb-3 flex flex-wrap items-center gap-2 text-xs">
        <span className="text-muted-foreground">Woken by</span>
        {MODES.map(({ mode, label }) => (
          <Button
            key={mode}
            size="sm"
            variant={overview.mode === mode ? "default" : "outline"}
            disabled={switching !== null}
            onClick={() => overview.mode !== mode && switchTo(mode)}
          >
            {switching === mode && <Loader2 className="size-3 animate-spin" />}
            {label}
          </Button>
        ))}
        {overview.mode === "off" && <Badge variant="outline">nothing runs by itself</Badge>}
      </div>
      {current && <p className="mb-2 text-xs text-muted-foreground">{current.hint}</p>}
      <div className="rounded-lg border px-3">
        {overview.jobs.map((view) => (
          <JobRow key={`${view.service}/${view.job}`} view={view} path={path} onChanged={refresh} />
        ))}
      </div>
    </div>
  );
};
