import { open as chooseFolder } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  AlertTriangle,
  Check,
  FolderOpen,
  HardDriveUpload,
  Loader2,
  ScrollText,
  X,
} from "lucide-react";

import { Alert } from "../../components/ui/alert";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../../components/ui/dialog";
import * as api from "../../api";
import type {
  BackupManifest,
  DbMethod,
  DeploymentRecord,
  RestoreEvent,
  RestoreOptions,
  RestorePlan,
  RestoreReport,
  Verdict,
} from "../../api";
import { cn } from "../../utils";

/**
 * Restore a backup into this hub.
 *
 * Four screens. *Choose* a folder and read its `manifest.json`. *Review* what the backup
 * holds against what this hub runs — the core does the comparison, this only renders it —
 * with the options, the warnings, anything that blocks, and the hub's name to type. Then
 * *run*, with a row per step as `BackupDialog` has. Then *done*: not "the files copied"
 * but a row per service saying whether it answers, because that is the question anyone
 * restoring a database actually has.
 */

type Phase = "choose" | "review" | "running" | "done" | "failed";

const STEPS: { key: string; title: string }[] = [
  { key: "preflight", title: "Checking the backup against this hub" },
  { key: "stop", title: "Stopping the hub" },
  { key: "volumes", title: "Data volumes" },
  { key: "postgres", title: "Database" },
  { key: "minio", title: "Object storage" },
  { key: "start", title: "Starting the hub" },
  { key: "health", title: "Checking the services" },
];

export type StepState = {
  status: "pending" | "running" | "done" | "skipped";
  last?: string;
};

const initialSteps = (): Record<string, StepState> =>
  Object.fromEntries(STEPS.map((s) => [s.key, { status: "pending" }]));

/** The narration folded into per-step state, plus the verdicts as they come in. */
export const reduceRestore = (
  steps: Record<string, StepState>,
  event: RestoreEvent
): Record<string, StepState> => {
  const next = { ...steps };
  if (event.event === "step") {
    for (const key of Object.keys(next)) {
      if (next[key].status === "running") next[key] = { ...next[key], status: "done" };
    }
    next[event.step] = { status: "running" };
  } else if (event.event === "line") {
    next[event.step] = { ...(next[event.step] ?? { status: "running" }), last: event.line };
  } else if (event.event === "skipped") {
    next[event.step] = { status: "skipped", last: event.reason };
  } else if (event.event === "checked") {
    next.health = {
      ...(next.health ?? { status: "running" }),
      last: `${event.service}: ${event.detail}`,
    };
  }
  return next;
};

const VERDICT: Record<Verdict, { label: string; tone: string }> = {
  same: { label: "same", tone: "text-muted-foreground" },
  "not-resolvable": { label: "same tag", tone: "text-muted-foreground" },
  "different-tag": { label: "different tag", tone: "text-warning" },
  "different-build": { label: "different build", tone: "text-warning" },
  "missing-in-target": { label: "not deployed here", tone: "text-destructive" },
};

const when = (secs: number) => new Date(secs * 1000).toLocaleString();

export const RestoreDialog = ({
  deployment,
  open,
  onOpenChange,
  onDone,
}: {
  deployment: DeploymentRecord;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** After a restore, successful or not — the dashboard reloads its containers. */
  onDone?: () => void;
}) => {
  const navigate = useNavigate();
  const [phase, setPhase] = useState<Phase>("choose");
  const [backup, setBackup] = useState<string | null>(null);
  const [manifest, setManifest] = useState<BackupManifest | null>(null);
  const [options, setOptions] = useState<RestoreOptions>({
    method: "dump",
    restore_postgres: true,
    restore_minio: true,
  });
  const [plan, setPlan] = useState<RestorePlan | null>(null);
  const [planning, setPlanning] = useState(false);
  const [typed, setTyped] = useState("");
  const [steps, setSteps] = useState<Record<string, StepState>>(initialSteps);
  const [lines, setLines] = useState<string[]>([]);
  const [report, setReport] = useState<RestoreReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const log = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    setPhase("choose");
    setBackup(null);
    setManifest(null);
    setPlan(null);
    setTyped("");
    setSteps(initialSteps());
    setLines([]);
    setReport(null);
    setError(null);
  }, [open]);

  // The plan follows the options: unticking the object storage, or switching to a raw
  // copy, changes what blocks and what warns.
  useEffect(() => {
    if (!backup || phase !== "review") return;
    let cancelled = false;
    setPlanning(true);
    api
      .restorePlan(deployment.path, backup, options)
      .then((next) => !cancelled && setPlan(next))
      .catch((e) => !cancelled && setError(typeof e === "string" ? e : String(e)))
      .finally(() => !cancelled && setPlanning(false));
    return () => {
      cancelled = true;
    };
  }, [backup, options, phase, deployment.path]);

  useEffect(() => {
    log.current?.scrollTo({ top: log.current.scrollHeight });
  }, [lines]);

  const pick = async () => {
    const chosen = await chooseFolder({
      directory: true,
      title: "Choose a backup folder (the one holding manifest.json)",
    });
    if (typeof chosen !== "string") return;
    setError(null);
    try {
      const read = await api.readBackupManifest(chosen);
      setBackup(chosen);
      setManifest(read);
      setPhase("review");
    } catch (e) {
      setBackup(null);
      setManifest(null);
      setError(typeof e === "string" ? e : String(e));
    }
  };

  const run = async () => {
    if (!backup) return;
    setPhase("running");
    setError(null);
    try {
      const result = await api.restoreDeployment(deployment.path, backup, options, (event) => {
        setSteps((previous) => reduceRestore(previous, event));
        if (event.event === "line") setLines((previous) => [...previous, event.line]);
        if (event.event === "skipped")
          setLines((previous) => [...previous, `skipped — ${event.reason}`]);
        if (event.event === "checked")
          setLines((previous) => [
            ...previous,
            `${event.healthy ? "✓" : "✗"} ${event.service}: ${event.detail}`,
          ]);
      });
      setSteps((previous) =>
        Object.fromEntries(
          Object.entries(previous).map(([key, state]) => [
            key,
            state.status === "running" ? { ...state, status: "done" } : state,
          ])
        )
      );
      setReport(result);
      setPhase("done");
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
      setPhase("failed");
    } finally {
      onDone?.();
    }
  };

  const busy = phase === "running";
  const canRun =
    plan !== null && !planning && plan.blocking.length === 0 && typed.trim() === deployment.name;

  return (
    <Dialog open={open} onOpenChange={(next) => !busy && onOpenChange(next)}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>Restore into {deployment.name}</DialogTitle>
          <DialogDescription>
            Replaces this hub's database and object storage with a backup's, then starts
            the hub and checks that every service still answers.
          </DialogDescription>
        </DialogHeader>

        {phase === "choose" && (
          <div className="flex flex-col gap-3">
            <div>
              <Button variant="outline" size="sm" onClick={() => void pick()}>
                <FolderOpen className="size-3.5" />
                Choose a backup folder
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              A backup folder is the one Konstruktor wrote — it holds{" "}
              <code>manifest.json</code>, which says which services and which versions
              the data came from.
            </p>
            {error && (
              <Alert variant="destructive">
                <AlertTriangle />
                {error}
              </Alert>
            )}
          </div>
        )}

        {phase === "review" && manifest && (
          <div className="flex flex-col gap-3">
            <div className="text-xs text-muted-foreground">
              Taken {when(manifest.taken_at)} from{" "}
              <span className="font-mono">
                {manifest.hub.identifier ?? "an unauthorized hub"}
              </span>
              {plan && (
                <span
                  className={cn(
                    "ml-2 rounded-full border px-2 py-0.5 text-[10px] uppercase tracking-wide",
                    plan.same_hub ? "border-border" : "border-warning text-warning"
                  )}
                  data-testid="same-hub"
                >
                  {plan.same_hub ? "this hub" : "a different hub"}
                </span>
              )}
            </div>

            <div className="overflow-x-auto rounded-md border border-border">
              <table className="w-full text-xs" data-testid="comparison">
                <thead className="bg-muted/40 text-muted-foreground">
                  <tr>
                    <th className="px-2 py-1 text-left font-medium">Service</th>
                    <th className="px-2 py-1 text-left font-medium">In the backup</th>
                    <th className="px-2 py-1 text-left font-medium">Deployed here</th>
                    <th className="px-2 py-1 text-left font-medium" />
                  </tr>
                </thead>
                <tbody>
                  {(plan?.services ?? []).map((row) => (
                    <tr key={row.host} className="border-t border-border">
                      <td className="px-2 py-1 font-medium">{row.host}</td>
                      <td className="px-2 py-1 font-mono">{row.backup_image}</td>
                      <td className="px-2 py-1 font-mono">{row.deployed_image ?? "—"}</td>
                      <td className={cn("px-2 py-1", VERDICT[row.verdict].tone)}>
                        {VERDICT[row.verdict].label}
                      </td>
                    </tr>
                  ))}
                  {(plan?.extra_in_target ?? []).map((id) => (
                    <tr key={id} className="border-t border-border text-muted-foreground">
                      <td className="px-2 py-1 font-medium">{id}</td>
                      <td className="px-2 py-1">—</td>
                      <td className="px-2 py-1">runs here</td>
                      <td className="px-2 py-1">nothing to restore</td>
                    </tr>
                  ))}
                  {plan && (
                    <tr className="border-t border-border">
                      <td className="px-2 py-1 font-medium">db</td>
                      <td className="px-2 py-1 font-mono">{plan.db.backup_image}</td>
                      <td className="px-2 py-1 font-mono">{plan.db.deployed_image}</td>
                      <td className={cn("px-2 py-1", VERDICT[plan.db.verdict].tone)}>
                        {VERDICT[plan.db.verdict].label}
                      </td>
                    </tr>
                  )}
                  {!plan && (
                    <tr>
                      <td colSpan={4} className="px-2 py-2 text-muted-foreground">
                        <Loader2 className="inline size-3 animate-spin" /> Comparing…
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>

            <div className="flex flex-col gap-2 text-sm">
              <label className="flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={options.restore_postgres}
                  onChange={(e) =>
                    setOptions((o) => ({ ...o, restore_postgres: e.target.checked }))
                  }
                />
                Database
                <select
                  className="ml-2 rounded border border-border bg-background px-1 py-0.5 text-xs"
                  value={options.method}
                  disabled={!options.restore_postgres}
                  aria-label="Database method"
                  onChange={(e) =>
                    setOptions((o) => ({ ...o, method: e.target.value as DbMethod }))
                  }
                >
                  <option value="dump">replay the SQL dump (recommended)</option>
                  <option value="raw">copy the raw files (same Postgres only)</option>
                </select>
              </label>
              <label className="flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={options.restore_minio}
                  onChange={(e) =>
                    setOptions((o) => ({ ...o, restore_minio: e.target.checked }))
                  }
                />
                Object storage
              </label>
            </div>

            {plan && plan.blocking.length > 0 && (
              <Alert variant="destructive" data-testid="blocking">
                <AlertTriangle />
                <ul className="flex flex-col gap-1">
                  {plan.blocking.map((reason) => (
                    <li key={reason}>{reason}</li>
                  ))}
                </ul>
              </Alert>
            )}
            {plan && plan.warnings.length > 0 && (
              <Alert data-testid="warnings">
                <AlertTriangle />
                <ul className="flex flex-col gap-1">
                  {plan.warnings.map((warning) => (
                    <li key={warning}>{warning}</li>
                  ))}
                </ul>
              </Alert>
            )}
            {error && (
              <Alert variant="destructive">
                <AlertTriangle />
                {error}
              </Alert>
            )}

            <label className="flex flex-col gap-1 text-xs text-muted-foreground">
              <span>
                This cannot be undone. Type <strong>{deployment.name}</strong> to confirm.
              </span>
              <Input
                value={typed}
                onChange={(e) => setTyped(e.target.value)}
                placeholder={deployment.name}
                aria-label="Type the hub's name to confirm"
                autoComplete="off"
              />
            </label>
          </div>
        )}

        {(phase === "running" || phase === "done" || phase === "failed") && (
          <div className="flex flex-col gap-3">
            <ul className="flex flex-col gap-1.5" data-testid="restore-steps">
              {STEPS.map((step) => {
                const state = steps[step.key] ?? { status: "pending" };
                return (
                  <li key={step.key} className="flex items-start gap-2 text-sm">
                    <span className="mt-0.5 flex size-4 shrink-0 items-center justify-center">
                      {state.status === "running" ? (
                        <Loader2 className="size-3.5 animate-spin text-primary" />
                      ) : state.status === "done" ? (
                        <Check className="size-3.5 text-primary" />
                      ) : state.status === "skipped" ? (
                        <AlertTriangle className="size-3.5 text-muted-foreground" />
                      ) : (
                        <span className="size-1.5 rounded-full bg-muted-foreground/40" />
                      )}
                    </span>
                    <div className="min-w-0 flex-1">
                      <div className={cn(state.status === "pending" && "text-muted-foreground")}>
                        {step.title}
                      </div>
                      {state.last && (
                        <div className="truncate font-mono text-[11px] text-muted-foreground">
                          {state.last}
                        </div>
                      )}
                    </div>
                  </li>
                );
              })}
            </ul>

            <div
              ref={log}
              className="max-h-32 overflow-y-auto rounded-md border border-border bg-muted/30 p-2 font-mono text-[11px] leading-relaxed text-muted-foreground"
            >
              {lines.length === 0 ? (
                <span>Starting…</span>
              ) : (
                lines.map((line, index) => <div key={index}>{line}</div>)
              )}
            </div>

            {error && (
              <Alert variant="destructive">
                <AlertTriangle />
                <div className="flex flex-col gap-1">
                  <span className="font-medium">The restore did not complete.</span>
                  <pre className="whitespace-pre-wrap font-mono text-xs">{error}</pre>
                </div>
              </Alert>
            )}

            {report && (
              <div className="flex flex-col gap-2" data-testid="health">
                <Alert variant={report.all_healthy ? "default" : "destructive"}>
                  {report.all_healthy ? <Check /> : <AlertTriangle />}
                  <span>
                    {report.all_healthy
                      ? `Restored, and all ${report.health.length} services answer.`
                      : `Restored, but ${report.health.filter((h) => !h.healthy).length} of ${
                          report.health.length
                        } services are not answering.`}
                  </span>
                </Alert>
                <ul className="flex flex-col gap-1 text-xs">
                  {report.health.map((h) => (
                    <li key={h.service} className="flex items-center gap-2">
                      <span
                        className={cn(
                          "size-2 shrink-0 rounded-full",
                          h.healthy ? "bg-success" : "bg-destructive"
                        )}
                      />
                      <span className="font-medium">{h.service}</span>
                      <span className="text-muted-foreground">{h.detail}</span>
                      {h.container_state && (
                        <span className="ml-auto font-mono text-muted-foreground">
                          {h.container_state}
                          {h.http_status !== null && ` · ${h.http_status}`}
                        </span>
                      )}
                    </li>
                  ))}
                </ul>
                {report.warnings.map((warning) => (
                  <span key={warning} className="text-xs text-muted-foreground">
                    {warning}
                  </span>
                ))}
              </div>
            )}
          </div>
        )}

        <DialogFooter>
          {phase === "choose" && (
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
          )}
          {phase === "review" && (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)}>
                Cancel
              </Button>
              <Button variant="destructive" disabled={!canRun} onClick={() => void run()}>
                <HardDriveUpload className="size-3.5" />
                Restore
              </Button>
            </>
          )}
          {phase === "running" && (
            <Button disabled>
              <Loader2 className="size-3.5 animate-spin" />
              Restoring…
            </Button>
          )}
          {(phase === "done" || phase === "failed") && (
            <>
              {report && !report.all_healthy && (
                <Button
                  variant="outline"
                  onClick={() => {
                    onOpenChange(false);
                    navigate(`/logs/${deployment.id}`);
                  }}
                >
                  <ScrollText className="size-3.5" />
                  Open logs
                </Button>
              )}
              <Button onClick={() => onOpenChange(false)}>
                {report?.all_healthy ? <Check className="size-3.5" /> : <X className="size-3.5" />}
                Close
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
