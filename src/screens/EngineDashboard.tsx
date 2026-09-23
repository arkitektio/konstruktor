import { open } from "@tauri-apps/plugin-shell";
import { ExternalLink, Puzzle, ScrollText, Server } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { TbReload } from "react-icons/tb";

import * as api from "../api";
import type { Container, DeploymentRecord, EngineAttachment, EngineMesh } from "../api";
import { HubPicker, useRegisteredHubs } from "../components/engine/HubPicker";
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
import { baseUrl } from "./deploy/hub-form";

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
  // Not `containers[0]`: an engine on the mesh has its sidecar beside it.
  const deployer = containers.find((c) => c.service === "deployer") ?? containers[0];

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

        <MeshCard deployment={deployment} />

        <HubAttachmentCard deployment={deployment} running={run.state !== "never" && run.state !== "stopped"} onChanged={load} />

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
                // A bare host means https; `http://localhost:8000` is taken as given.
                onClick={() => open(baseUrl(deployment.coordServer ?? ""))}
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

/**
 * The engine's place on the mesh, when it has one: the name it joins as, and whether its
 * sidecar is connected right now. Absent for an engine created without the mesh.
 */
const MeshCard = ({ deployment }: { deployment: DeploymentRecord }) => {
  const [mesh, setMesh] = useState<EngineMesh | null>(null);

  useEffect(() => {
    let cancelled = false;
    const read = () =>
      api
        .engineMesh(deployment.path)
        .then((m) => !cancelled && setMesh(m))
        .catch(() => undefined);
    void read();
    const timer = setInterval(read, 10000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [deployment.path]);

  if (!mesh) return null;
  const live = mesh.live;
  return (
    <div>
      <SectionHeading hint="A Tailscale container runs beside the engine, and plugins run inside its network, so they reach the hub the engine is bound to over the tailnet.">
        Mesh
      </SectionHeading>
      <Card className="gap-0 py-4 px-4 border-border max-w-2xl">
        <div className="flex items-center gap-2 text-sm">
          <span
            className={cn(
              "size-2 rounded-full",
              live?.connected ? "bg-green-500" : live ? "bg-amber-500" : "bg-muted-foreground"
            )}
          />
          {live?.connected
            ? `Connected as ${live.hostname ?? live.ipv4 ?? mesh.hostname}`
            : live
              ? "Not connected"
              : `Joins as ${mesh.hostname} — not running`}
        </div>
      </Card>
    </div>
  );
};

/**
 * Which hub's network the engine joins. Attached, the engine and the plugins it starts
 * sit on the hub's network and reach it as `gateway`, without leaving Docker.
 */
const HubAttachmentCard = ({
  deployment,
  running,
  onChanged,
}: {
  deployment: DeploymentRecord;
  running: boolean;
  onChanged: () => Promise<void>;
}) => {
  const hubs = useRegisteredHubs();
  const [attachment, setAttachment] = useState<EngineAttachment | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const read = useCallback(async () => {
    const current = await api.engineAttachment(deployment.path);
    setAttachment(current);
    setChosen(current?.hub_path ?? null);
  }, [deployment.path]);

  useEffect(() => {
    read().catch((e) => setError(String(e)));
  }, [read]);

  const current = attachment?.hub_path ?? null;
  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.attachEngine(deployment.path, chosen);
      // `up` recreates the deployer on its new networks; a stopped engine picks it up
      // when it is next started.
      if (running) await api.composeCommand(deployment.path, "up");
      await read();
      await onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <SectionHeading hint="Attached to a hub, the engine joins that hub's network, and so does every plugin it starts: they reach the hub from inside Docker, as `gateway`. The hub has to be running for the engine to start.">
        Hub network
      </SectionHeading>

      <Card className="gap-3 py-4 px-4 border-border max-w-2xl">
        <div className="text-sm">
          {attachment
            ? attachment.hub_name
              ? <>Attached to <span className="font-medium">{attachment.hub_name}</span></>
              : <>Joins the network <code>{attachment.network}</code>, which no registered hub owns any more.</>
            : "Not attached — plugins run on the engine's own network."}
        </div>
        <div className="flex items-center gap-2">
          <div className="flex-1 min-w-0">
            <HubPicker hubs={hubs} value={chosen} onChange={setChosen} disabled={busy} />
          </div>
          <Button
            variant="outline"
            size="sm"
            disabled={busy || chosen === current}
            onClick={() => void apply()}
          >
            {busy ? "Applying…" : chosen ? "Attach" : "Detach"}
          </Button>
        </div>
        {attachment && running && (
          <div className="text-xs text-muted-foreground">
            Plugins already running stay on the network they were started on until they are
            restarted.
          </div>
        )}
        {error && <div className="text-xs text-destructive">{error}</div>}
      </Card>
    </div>
  );
};
