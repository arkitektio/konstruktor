import { HardDriveDownload, Loader2, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Alert } from "../../components/ui/alert";
import { Button } from "../../components/ui/button";
import * as api from "../../api";
import type { InfrastructureUpdates as Pending, UpdateReport } from "../../api";

/**
 * Newer versions of the infrastructure — database, cache, object storage, gateway, mesh
 * sidecar — offered separately from the services' own update buttons.
 *
 * They are held back there on purpose: a service migrates its own schema forward, while a
 * moved database image can be a cluster the new binary will not open. So this is what
 * `konstruktor update --infra` does, asked for explicitly: a backup first, pins rewritten
 * where a newer version is published, the guard on the database, and a health check at
 * the end — all in the core's `updates::apply`.
 */
export const InfrastructureUpdates = ({
  path,
  stackUp,
  onUpdated,
}: {
  path: string;
  stackUp: boolean;
  onUpdated: () => void;
}) => {
  const [pending, setPending] = useState<Pending | undefined>();
  const [backupInto, setBackupInto] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [running, setRunning] = useState(false);
  const [step, setStep] = useState<string | undefined>();
  const [report, setReport] = useState<UpdateReport | undefined>();
  const [error, setError] = useState<string | undefined>();

  // Asked once the stack is up: it is registry traffic, and a stopped hub has nothing
  // running to update.
  useEffect(() => {
    if (!stackUp) return;
    let cancelled = false;
    api
      .infrastructureUpdates(path)
      .then((next) => !cancelled && setPending(next))
      .catch(() => undefined);
    api.defaultBackupFolder(path).then((folder) => !cancelled && setBackupInto(folder));
    return () => {
      cancelled = true;
    };
  }, [path, stackUp]);

  const apply = useCallback(async () => {
    if (!pending) return;
    setConfirming(false);
    setRunning(true);
    setError(undefined);
    try {
      const done = await api.applyUpdate(
        path,
        {
          services: pending.moved,
          advances: pending.advances,
          pull: true,
          backup_into: backupInto,
          health_check: true,
        },
        (event) => {
          if (event.event === "step") setStep(event.title);
        }
      );
      setReport(done);
      setPending(undefined);
      onUpdated();
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setRunning(false);
      setStep(undefined);
    }
  }, [pending, path, backupInto, onUpdated]);

  if (report) {
    const sick = report.health?.filter((h) => !h.healthy) ?? [];
    return (
      <Alert variant={report.refused.length || sick.length ? "destructive" : "default"}>
        <div className="flex flex-col gap-1 text-sm">
          {report.updated.length > 0 && <span>Updated {report.updated.join(", ")}.</span>}
          {report.refused.map(([service, reason]) => (
            <span key={service}>
              {service} was left alone: {reason}
            </span>
          ))}
          {sick.map((h) => (
            <span key={h.service}>
              {h.service} does not answer: {h.detail}
            </span>
          ))}
          {report.backup && (
            <span className="text-muted-foreground">
              The backup taken first is at <code>{report.backup}</code>.
            </span>
          )}
        </div>
      </Alert>
    );
  }

  if (!pending || (pending.moved.length === 0 && pending.advances.length === 0)) {
    return null;
  }

  return (
    <div className="mt-3 rounded-md border px-3 py-2 flex flex-col gap-2 text-sm">
      <div className="font-medium">Infrastructure updates</div>
      {pending.moved.length > 0 && (
        <div className="text-muted-foreground">
          Newer images: <span className="font-mono">{pending.moved.join(", ")}</span>
        </div>
      )}
      {pending.advances.map((advance) => (
        <div key={advance.service} className="text-muted-foreground font-mono text-xs">
          {advance.from} → {advance.to}
        </div>
      ))}
      {error && <div className="text-destructive">{error}</div>}
      {confirming && (
        <Alert>
          <TriangleAlert />
          <div>
            A backup is taken into{" "}
            <code>{backupInto ?? "the folder beside this deployment"}</code> first. The
            database is checked before it moves: a new Postgres major is refused, not
            applied.
          </div>
        </Alert>
      )}
      <div>
        {running ? (
          <Button size="sm" variant="outline" disabled>
            <Loader2 className="size-3.5 animate-spin" />
            {step ?? "Updating…"}
          </Button>
        ) : confirming ? (
          <div className="flex gap-2">
            <Button size="sm" onClick={() => void apply()}>
              <HardDriveDownload className="size-3.5" />
              Back up and update
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setConfirming(false)}>
              Cancel
            </Button>
          </div>
        ) : (
          <Button size="sm" variant="outline" onClick={() => setConfirming(true)}>
            Update infrastructure…
          </Button>
        )}
      </div>
    </div>
  );
};
