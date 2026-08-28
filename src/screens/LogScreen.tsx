import { Loader2, RefreshCw, ScrollText } from "lucide-react";
import React, { useCallback, useEffect, useState } from "react";
import { useParams } from "react-router-dom";

import { AppMenu } from "../components/AppMenu";
import { Alert } from "../components/ui/alert";
import { Button } from "../components/ui/button";
import * as api from "../api";
import { Page } from "../layout/Page";
import { PageHeader } from "../layout/PageHeader";
import type { DeploymentRecord } from "../api";
import { useRegistry } from "../registry/registry-context";
import { cn } from "../utils";

export const Logs: React.FC<{
  deployment: DeploymentRecord;
  service?: string;
}> = ({ deployment, service }) => {
  const [logs, setLogs] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  const reload = useCallback(() => {
    setRunning(true);
    setError(null);
    // `docker compose logs` on a stack that was never started exits 0 with nothing to
    // say, so an empty result is not an error — but a docker that cannot be reached is.
    api
      .composeCommand(deployment.path, "logs", { service, tail: 200 })
      .then((output) => setLogs(output.split("\n").filter((l) => l.length > 0)))
      .catch((e) => {
        setLogs([]);
        setError(typeof e === "string" ? e : String(e));
      })
      .finally(() => setRunning(false));
  }, [deployment.path, service]);

  useEffect(() => {
    reload();
  }, [reload]);

  return (
    <Page
      menu={
        <AppMenu
          back={`/dashboard/${deployment.id}`}
          breadcrumb={
            service ? `${deployment.name} · ${service}` : `${deployment.name} · logs`
          }
        />
      }
    >
      <div className="flex flex-col gap-4">
        <PageHeader
          icon={ScrollText}
          title="Logs"
          subtitle={service ? `${deployment.name} · ${service}` : deployment.name}
          actions={
            <Button
              variant="outline"
              size="sm"
              disabled={running}
              onClick={() => reload()}
            >
              <RefreshCw className={cn("size-3.5", running && "animate-spin")} />
              {running ? "Reading…" : "Reload"}
            </Button>
          }
        />
        {error && (
          <Alert variant="destructive" className="max-w-2xl">
            {error}
          </Alert>
        )}

        <div className="rounded-lg border border-border bg-card p-3 overflow-x-auto">
          <pre className="text-xs leading-relaxed font-mono whitespace-pre">
            {logs.length > 0 ? (
              logs.map((line, index) => <div key={index}>{line}</div>)
            ) : running ? (
              <span className="text-muted-foreground inline-flex items-center gap-2">
                <Loader2 className="size-3.5 animate-spin" />
                Reading the logs…
              </span>
            ) : (
              <span className="text-muted-foreground">
                Nothing yet — this deployment has not written any logs. Start it from
                the dashboard if it is not running.
              </span>
            )}
          </pre>
        </div>
      </div>
    </Page>
  );
};

export const LogScreen: React.FC<{}> = () => {
  const { id, service } = useParams<{ id: string; service: string }>();
  const { byId } = useRegistry();

  const deployment = id ? byId(id) : undefined;

  return deployment ? (
    <Logs deployment={deployment} service={service} />
  ) : (
    <>Could not find this deployment</>
  );
};
