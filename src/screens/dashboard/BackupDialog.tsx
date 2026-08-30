import { open as chooseFolder } from "@tauri-apps/plugin-dialog";
import { open as reveal } from "@tauri-apps/plugin-shell";
import { useEffect, useRef, useState } from "react";
import { AlertTriangle, Check, FolderOpen, HardDriveDownload, Loader2 } from "lucide-react";

import { Alert } from "../../components/ui/alert";
import { Button } from "../../components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../../components/ui/dialog";
import * as api from "../../api";
import type { BackupEvent, BackupReport, DeploymentRecord, StorageMode } from "../../api";
import { cn } from "../../utils";

/**
 * Back the hub's data up into a folder.
 *
 * Two screens in one dialog: pick where, then watch it go. The narration matters more
 * here than for a compose command — a backup of a busy hub is a long copy with several
 * distinct parts, and "working…" for four minutes reads as "hung". So each part gets a
 * row that turns from pending to running to done, with the last thing it said beside it.
 *
 * What the backup consists of, and why the copy runs inside a container, is documented
 * on `konstruktor_core::backup`; the copy at the top of the dialog is the short version.
 */

type Phase = "choose" | "running" | "done" | "failed";

const STEPS: { key: string; title: string }[] = [
  { key: "deployment", title: "Configuration" },
  { key: "dump", title: "Database dump (pg_dumpall)" },
  { key: "postgres", title: "Database files" },
  { key: "minio", title: "Object storage" },
  { key: "manifest", title: "Manifest" },
];

type StepState = {
  status: "pending" | "running" | "done" | "skipped";
  last?: string;
};

const initialSteps = (): Record<string, StepState> =>
  Object.fromEntries(STEPS.map((s) => [s.key, { status: "pending" }]));

/** The narration folded into per-step state. Exported for the test. */
export const reduceBackup = (
  steps: Record<string, StepState>,
  event: BackupEvent
): Record<string, StepState> => {
  const next = { ...steps };
  // A step starting means whichever step was running before has finished.
  if (event.event === "step") {
    for (const key of Object.keys(next)) {
      if (next[key].status === "running") next[key] = { ...next[key], status: "done" };
    }
    next[event.step] = { status: "running" };
  } else if (event.event === "line") {
    next[event.step] = { ...(next[event.step] ?? { status: "running" }), last: event.line };
  } else if (event.event === "skipped") {
    next[event.step] = { status: "skipped", last: event.reason };
  }
  return next;
};

export const BackupDialog = ({
  deployment,
  storage,
  open,
  onOpenChange,
}: {
  deployment: DeploymentRecord;
  storage?: StorageMode;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) => {
  const [phase, setPhase] = useState<Phase>("choose");
  const [target, setTarget] = useState<string | null>(null);
  const [folder, setFolder] = useState<string | null>(null);
  const [steps, setSteps] = useState<Record<string, StepState>>(initialSteps);
  const [lines, setLines] = useState<string[]>([]);
  const [report, setReport] = useState<BackupReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const log = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    setPhase("choose");
    setTarget(null);
    setFolder(null);
    setSteps(initialSteps());
    setLines([]);
    setReport(null);
    setError(null);
  }, [open]);

  useEffect(() => {
    if (!target) return;
    api.backupFolder(deployment.path, target).then(setFolder).catch(() => setFolder(null));
  }, [target, deployment.path]);

  useEffect(() => {
    log.current?.scrollTo({ top: log.current.scrollHeight });
  }, [lines]);

  const pick = async () => {
    const chosen = await chooseFolder({
      directory: true,
      title: "Choose a folder to back up into",
    });
    if (typeof chosen === "string") setTarget(chosen);
  };

  const run = async () => {
    if (!target) return;
    setPhase("running");
    setError(null);
    try {
      const result = await api.backupDeployment(deployment.path, target, (event) => {
        setSteps((previous) => reduceBackup(previous, event));
        if (event.event === "line") setLines((previous) => [...previous, event.line]);
        if (event.event === "skipped")
          setLines((previous) => [...previous, `skipped — ${event.reason}`]);
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
    }
  };

  const busy = phase === "running";

  return (
    <Dialog open={open} onOpenChange={(next) => !busy && onOpenChange(next)}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Back up {deployment.name}</DialogTitle>
          <DialogDescription>
            A <code>pg_dumpall</code> of the database, a copy of its files, a copy of the
            object storage, and the hub's configuration, in a timestamped folder — with a{" "}
            <code>manifest.json</code> naming the services and versions it came from, so it
            can be restored later and checked against whatever runs then.
            {storage === "docker-volumes" && (
              <>
                {" "}
                The data lives in Docker volumes, so the copy runs in a small container
                with <code>rsync</code> — it is the only way to reach them.
              </>
            )}
          </DialogDescription>
        </DialogHeader>

        {phase === "choose" && (
          <div className="flex flex-col gap-3">
            <div className="flex items-center gap-2">
              <Button variant="outline" size="sm" onClick={() => void pick()}>
                <FolderOpen className="size-3.5" />
                {target ? "Change folder" : "Choose a folder"}
              </Button>
              {target && (
                <span className="font-mono text-xs break-all text-muted-foreground">
                  {target}
                </span>
              )}
            </div>
            {folder && (
              <p className="text-xs text-muted-foreground">
                Will be written to <span className="font-mono break-all">{folder}</span>
              </p>
            )}
            <p className="text-xs text-muted-foreground">
              The database has to be running for the dump. If it is not, it is started
              for the dump and stopped again afterwards. A hub that is up keeps running
              throughout.
            </p>
          </div>
        )}

        {phase !== "choose" && (
          <div className="flex flex-col gap-3">
            <ul className="flex flex-col gap-1.5" data-testid="backup-steps">
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
                      <div
                        className={cn(
                          state.status === "pending" && "text-muted-foreground"
                        )}
                      >
                        {step.title}
                        {state.status === "skipped" && (
                          <span className="ml-2 text-xs text-muted-foreground">skipped</span>
                        )}
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
              className="max-h-40 overflow-y-auto rounded-md border border-border bg-muted/30 p-2 font-mono text-[11px] leading-relaxed text-muted-foreground"
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
                  <span className="font-medium">The backup did not complete.</span>
                  <pre className="whitespace-pre-wrap font-mono text-xs">{error}</pre>
                </div>
              </Alert>
            )}

            {report && (
              <Alert data-testid="backup-done">
                <Check />
                <div className="flex flex-col gap-1">
                  <span>
                    Written to{" "}
                    <span className="font-mono break-all">{report.path}</span>
                  </span>
                  {report.warnings.map((warning) => (
                    <span key={warning} className="text-xs">
                      {warning}
                    </span>
                  ))}
                </div>
              </Alert>
            )}
          </div>
        )}

        <DialogFooter>
          {phase === "choose" && (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)}>
                Cancel
              </Button>
              <Button disabled={!target} onClick={() => void run()}>
                <HardDriveDownload className="size-3.5" />
                Back up
              </Button>
            </>
          )}
          {phase === "running" && (
            <Button disabled>
              <Loader2 className="size-3.5 animate-spin" />
              Backing up…
            </Button>
          )}
          {(phase === "done" || phase === "failed") && (
            <>
              {report && (
                <Button variant="outline" onClick={() => void reveal(report.path)}>
                  <FolderOpen className="size-3.5" />
                  Open folder
                </Button>
              )}
              <Button onClick={() => onOpenChange(false)}>Close</Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
