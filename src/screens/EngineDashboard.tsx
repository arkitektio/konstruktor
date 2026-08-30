import { open } from "@tauri-apps/plugin-shell";
import { ExternalLink, Puzzle, ScrollText, Server } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { TbReload } from "react-icons/tb";

import * as api from "../api";
import type { Container, DeploymentRecord } from "../api";
import { AppMenu } from "../components/AppMenu";
import { CommandButton, DangerousCommandButton } from "../CommandButton";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card } from "../components/ui/card";
import { Page } from "../layout/Page";
import { PageHeader, SectionHeading } from "../layout/PageHeader";
import { useCommunication } from "../communication/communication-context";
import { EngineSetupPanel } from "../components/engine/EngineSetupPanel";
import { DeploymentMenu } from "./dashboard/DeploymentMenu";
import { RUN_STATE_DOT } from "./dashboard/tone";
import { RUN_STATE_LABEL, runSummary } from "./dashboard/lifecycle";
import { cn } from "../utils";

/**
 * A plugin engine's dashboard, which is not the hub's.
 *
 * A hub dashboard is built around a profile: a dozen services, their channels, their
 * images, an admin account, a lifecycle that runs from "created" through "authorized" to
 * "started". An engine has none of that. It is one container whose whole job is to be
 * running and hold the Docker socket, so the page answers the two questions that exist —
 * is it up, and what is it — and gets out of the way.
 */
export const EngineDashboard = ({ deployment }: { deployment: DeploymentRecord }) => {
  const [containers, setContainers] = useState<Container[]>([]);
  const { state: engineState } = useCommunication();
  // The daemon went away — or was never there. An empty container list would read as
  // "not started"; this reads as what it is.
  const engineDown = engineState !== "ready" && engineState !== "checking";

  const load = useCallback(async () => {
    try {
      const result = await api.listDeploymentContainers(deployment.path);
      setContainers(result.containers);
    } catch (e) {
      // The daemon is not always reachable; the compose buttons still work.
      console.error("Could not list the engine's containers", e);
      setContainers([]);
    }
  }, [deployment.path]);

  useEffect(() => {
    void load();
    const timer = setInterval(load, 3000);
    return () => clearInterval(timer);
  }, [load]);

  const run = useMemo(() => runSummary(containers), [containers]);
  const deployer = containers[0];

  const restart = async () => {
    if (!deployer?.id) return;
    await api.restartContainer(deployer.id);
    await load();
  };

  return (
    <Page
      menu={<AppMenu back="/" breadcrumb={deployment.name} />}
      buttons={
        <>
          <CommandButton
            title={run.state === "running" ? "Recreate" : "Start"}
            runningTitle="Starting…"
            path={deployment.path}
            action="up"
            callback={load}
          />
          <DangerousCommandButton
            title="Stop"
            runningTitle="Stopping…"
            confirmTitle="Stop this engine?"
            confirmDescription="Plugins it started keep running; the engine will not react to anything until it is started again."
            path={deployment.path}
            action="stop"
            callback={load}
          />
        </>
      }
    >
      <div className="flex flex-col gap-8">
        <PageHeader
          icon={Puzzle}
          title={deployment.name}
          badge={
            <span className="flex items-center gap-2">
              <Badge variant="outline" className="font-normal">
                Plugin engine
              </Badge>
              <Badge variant="outline" className="font-normal gap-1.5">
                <span className={cn("size-2 rounded-full", RUN_STATE_DOT[run.state])} />
                {RUN_STATE_LABEL[run.state]}
              </Badge>
            </span>
          }
          subtitle={
            <span className="block max-w-[52ch] truncate" title={deployment.path}>
              {deployment.path}
            </span>
          }
          actions={
            <DeploymentMenu
              deployment={deployment}
              onRefresh={load}
              onReload={load}
            />
          }
        />

        {engineDown && (
          <div className="max-w-2xl">
            <EngineSetupPanel />
          </div>
        )}

        <div>
          <SectionHeading hint="The one container this deployment is. It holds this machine's Docker socket, which is how it starts and stops plugins.">
            The engine
          </SectionHeading>

          <Card className="gap-0 py-4 px-4 border-border max-w-2xl">
            <div className="flex items-center gap-3">
              <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted">
                <Puzzle className="size-4" />
              </span>
              <div className="min-w-0 flex-1">
                <div className="font-medium truncate">
                  {deployer?.names?.[0]?.replace(/^\//, "") ?? "deployer"}
                </div>
                <div className="text-xs text-muted-foreground truncate">
                  {deployer
                    ? `${deployer.status ?? deployer.state} · ${deployer.image ?? ""}`
                    : "Not running — nothing has been created on the daemon yet."}
                </div>
              </div>
              <Button variant="outline" size="sm" asChild>
                <Link to={`/logs/${deployment.id}`}>
                  <ScrollText className="size-3.5" />
                  Logs
                </Link>
              </Button>
              <Button
                variant="ghost"
                size="sm"
                disabled={!deployer?.id}
                title="Restart the engine"
                onClick={() => void restart()}
              >
                <TbReload />
              </Button>
            </div>
          </Card>
        </div>

        <div>
          <SectionHeading hint="Where this engine belongs. It is configured against a coordination server, and plugins are installed through the organization there rather than from here.">
            Coordination
          </SectionHeading>

          <Card className="gap-0 py-4 px-4 border-border max-w-2xl">
            <div className="grid grid-cols-3 gap-2 text-sm">
              <div className="text-muted-foreground flex items-center gap-2">
                <Server className="size-3.5" />
                Server
              </div>
              <div className="col-span-2 break-all">
                {deployment.coordServer ?? "—"}
              </div>
              <div className="text-muted-foreground">Identifier</div>
              <div className="col-span-2 break-all">
                {deployment.identifier ?? "—"}
              </div>
            </div>

            {deployment.coordServer && (
              <Button
                variant="outline"
                size="sm"
                className="mt-3 self-start"
                onClick={() => open(`https://${deployment.coordServer}`)}
              >
                <ExternalLink className="size-3.5" />
                Open the coordination server
              </Button>
            )}
          </Card>
        </div>
      </div>
    </Page>
  );
};
