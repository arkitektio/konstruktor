import { open } from "@tauri-apps/plugin-shell";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { Boxes, ExternalLink, ShieldCheck } from "lucide-react";

import { CommandButton, DangerousCommandButton } from "../CommandButton";
import { AppMenu } from "../components/AppMenu";
import { Alert } from "../components/ui/alert";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Page } from "../layout/Page";
import { PageHeader, SectionHeading } from "../layout/PageHeader";
import { ResponsiveGrid } from "../layout/ResponsiveGrid";
import { cn } from "../utils";

import * as api from "../api";
import { useRegistry } from "../registry/registry-context";
import type { Container, DeploymentRecord, HubStatus, ImageState } from "../api";
import { AdminCard } from "./dashboard/AdminCard";
import { ChannelCard } from "./dashboard/ChannelCard";
import { CheckoutsCard, useCheckouts } from "./dashboard/CheckoutsCard";
import { DeploymentMenu } from "./dashboard/DeploymentMenu";
import { EngineDashboard } from "./EngineDashboard";
import { InfrastructureRow } from "./dashboard/InfrastructureRow";
import { LifecycleRail } from "./dashboard/LifecycleRail";
import { ServiceCard } from "./dashboard/ServiceCard";
import { UpdatesCard } from "./dashboard/UpdatesCard";
import {
  runSummary,
  serviceUpdates,
  stages,
  updateSummary,
  RUN_STATE_LABEL,
} from "./dashboard/lifecycle";
import { RUN_STATE_DOT } from "./dashboard/tone";

/**
 * The management view of a deployment.
 *
 * Everything shown here is derived from the profile YAML in the folder, and from the
 * compose containers running out of the deployment folder. The stack itself carries no
 * Konstruktor-specific labels — it is an ordinary compose project, and can be driven
 * from a terminal in that folder just as well.
 *
 * The page is organised around the deployment's life rather than around its parts: the
 * rail at the top answers "how far has this got" (created, authorized, configured,
 * started), the two cards under it answer "what is it following" and "is anything
 * waiting", and only then does it list services. The rules behind all of that live in
 * `dashboard/lifecycle.ts`, deliberately away from the JSX.
 *
 * The page itself only *reports*. Acting on the deployment happens in two places and no
 * others: Start/Recreate and Stop in the footer, because that is what people come here to
 * press, and everything else — folder, logs, authorize, and the three destructive ones —
 * behind the header's menu in `dashboard/DeploymentMenu.tsx`. There used to be a fourth
 * place, a "Danger zone" block at the very bottom, which put "Delete all data" one
 * mis-scroll away from the rest of the page.
 */

export const Dashboard = ({ deployment }: { deployment: DeploymentRecord }) => {
  const [status, setStatus] = useState<HubStatus | undefined>();
  const [profileError, setProfileError] = useState<string | undefined>();
  const [containers, setContainers] = useState<Container[]>([]);
  const [images, setImages] = useState<ImageState[]>([]);
  const { checkouts, reload: reloadCheckouts, replace: replaceCheckout } = useCheckouts(deployment.path);
  /** A branch moved since the stack was last brought up, so the code on disk is ahead. */
  const [switched, setSwitched] = useState(false);

  /**
   * A hub profile, for the deployments that have one.
   *
   * A plugin engine has no `hub_config.yaml` — it is one deployer container with the
   * Docker socket — so asking for one and then complaining that it is missing would put
   * a red alert on every engine dashboard. Everything that reads `status` is guarded on
   * it already; this just stops the question being asked.
   */
  const isHub = deployment.kind !== "engine";

  const loadProfile = useCallback(async () => {
    if (!isHub) {
      setStatus(undefined);
      setProfileError(undefined);
      return;
    }
    try {
      setStatus(await api.hubStatus(deployment.path));
      setProfileError(undefined);
    } catch (e) {
      setProfileError(e instanceof Error ? e.message : String(e));
    }
  }, [deployment.path, isHub]);

  useEffect(() => {
    loadProfile();
  }, [loadProfile]);

  const loadContainers = useCallback(async () => {
    try {
      const result = await api.listDeploymentContainers(deployment.path);
      setContainers(result.containers);
    } catch (e) {
      // The docker socket is not always reachable (a remote daemon, or no permission);
      // the compose commands still work, so this is not worth shouting about.
      console.error("Could not list containers", e);
      setContainers([]);
    }
  }, [deployment.path]);

  /**
   * Inspecting every image is a round trip per image, so it runs far less often than the
   * container poll — and on demand after a pull, which is the only thing that changes it.
   */
  const loadImages = useCallback(async () => {
    try {
      setImages(await api.deploymentImages(deployment.path));
    } catch (e) {
      console.error("Could not inspect the stack's images", e);
      setImages([]);
    }
  }, [deployment.path]);

  useEffect(() => {
    loadContainers();
    const timer = setInterval(loadContainers, 3000);
    return () => clearInterval(timer);
  }, [loadContainers]);

  useEffect(() => {
    loadImages();
    const timer = setInterval(loadImages, 30000);
    return () => clearInterval(timer);
  }, [loadImages]);

  const services = status?.services ?? [];

  const byService = useMemo(() => {
    const grouped = new Map<string, Container[]>();
    for (const container of containers) {
      const key = container.service ?? "";
      grouped.set(key, [...(grouped.get(key) ?? []), container]);
    }
    return grouped;
  }, [containers]);

  /** Everything the profile does not list as a service: db, redis, minio, the gateway. */
  const infrastructure = useMemo(() => {
    const hosts = new Set(services.map((s) => s.host));
    return containers.filter((c) => !c.service || !hosts.has(c.service));
  }, [containers, services]);

  const run = useMemo(() => runSummary(containers), [containers]);
  const updates = useMemo(
    () => serviceUpdates(images, containers),
    [images, containers]
  );
  const updatesByService = useMemo(
    () => new Map(updates.map((u) => [u.service, u])),
    [updates]
  );
  const rail = useMemo(
    () => stages(deployment, status, run),
    [deployment, status, run]
  );
  const pending = updateSummary(updates);

  const url = status?.gateway_url;

  const refreshAll = () => {
    void loadContainers();
    void loadImages();
    void reloadCheckouts();
    // Recreating is what makes the containers run the checked-out code, and it is the
    // only thing that clears the notice.
    setSwitched(false);
  };

  /** The header menu's "Reload": the profile as well as everything derived from it. */
  const reloadAll = () => {
    void loadProfile();
    refreshAll();
  };

  const restart = async (id: string) => {
    await api.restartContainer(id);
    await loadContainers();
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
            callback={refreshAll}
          />
          <DangerousCommandButton
            title="Stop"
            runningTitle="Stopping…"
            confirmTitle="Stop this deployment?"
            confirmDescription="Every service will be shut down. Your data stays where it is."
            path={deployment.path}
            action="stop"
            callback={refreshAll}
          />
        </>
      }
    >
      {/* Kontrol's page rhythm: header, separator, then blocks a fixed gap apart. */}
      <div className="flex flex-col gap-8">
        <PageHeader
          icon={Boxes}
          title={deployment.name}
          badge={
            <span className="flex items-center gap-2">
              <Badge variant="outline" className="font-normal">
                Hub
              </Badge>
              <Badge variant="outline" className="font-normal gap-1.5">
                <span
                  className={cn("size-2 rounded-full", RUN_STATE_DOT[run.state])}
                />
                {RUN_STATE_LABEL[run.state]}
              </Badge>
            </span>
          }
          subtitle={
            <span className="flex flex-wrap items-center gap-x-3 gap-y-1">
              {/* `truncate` needs a definite width, which a wrapping flex item does not
                  have — hence the explicit cap, so a deep path ellipses rather than
                  pushing the row wider than the window. */}
              <span className="block max-w-[52ch] truncate" title={deployment.path}>
                {deployment.path}
              </span>
              {status?.authorized && (
                <span className="flex items-center gap-1.5">
                  <ShieldCheck className="size-3.5" />
                  {status.identifier ?? "authorized"} on{" "}
                  {status.profile.config.coord_server}
                </span>
              )}
              {status?.mesh_hostname && <span>mesh · {status.mesh_hostname}</span>}
            </span>
          }
          actions={
            <>
              {url && (
                <Button variant="outline" size="sm" onClick={() => open(url)}>
                  <ExternalLink className="size-3.5" />
                  Open {url}
                </Button>
              )}
              <DeploymentMenu
                deployment={deployment}
                url={url}
                onRefresh={refreshAll}
                onReload={reloadAll}
              />
            </>
          }
        />

        {profileError && (
          <Alert variant="destructive" className="max-w-2xl">
            Could not read this deployment's configuration: {profileError}
          </Alert>
        )}

        <div>
          <SectionHeading hint="Where this deployment has got to, read off its folder and its containers.">
            Lifecycle
          </SectionHeading>
          <LifecycleRail stages={rail} />
        </div>

        {/*
          The updates card stays outside the profile guard: it needs only the images and
          the folder, and it carries the one button that pulls them. A hub whose profile
          cannot be read is exactly the one you might want to pull images for.
        */}
        {checkouts.length > 0 && (
          <div className="flex flex-col gap-3">
            <CheckoutsCard
              path={deployment.path}
              checkouts={checkouts}
              onChanged={(next) => {
                replaceCheckout(next);
                setSwitched(true);
              }}
            />
            {switched && (
              <Alert className="max-w-2xl text-sm">
                The checkout moved, but the containers are still running the code they
                started with. Recreate the stack to pick it up.
              </Alert>
            )}
          </div>
        )}

        <div className="grid gap-4 grid-cols-1 lg:grid-cols-2">
          {status && <ChannelCard status={status} updates={updates} />}
          {/*
            Only when something is actually waiting. A card that says "nothing waiting"
            every day of the week is noise, and it was the loudest thing on the page.
            Pulling — the only way to find out whether something newer exists — moved to
            the deployment menu, where it is available whether or not this card is here.
          */}
          {(pending.pulled.length > 0 || pending.missing.length > 0) && (
            <UpdatesCard
              updates={updates}
              onRefresh={refreshAll}
              path={deployment.path}
            />
          )}
        </div>

        {services.length > 0 && (
          <div>
            <SectionHeading
              hint={
                pending.pulled.length > 0
                  ? `${pending.pulled.length} of these are running an image older than the one on disk.`
                  : undefined
              }
            >
              Services
            </SectionHeading>
            <ResponsiveGrid>
              {services.map((service) => (
                <ServiceCard
                  key={service.id}
                  service={service}
                  containers={byService.get(service.host) ?? []}
                  deployment={deployment}
                  update={updatesByService.get(service.host)}
                  onRestart={restart}
                />
              ))}
            </ResponsiveGrid>
          </div>
        )}

        {infrastructure.length > 0 && (
          <div>
            <SectionHeading hint="The database, cache, object storage and gateway the services run on.">
              Infrastructure
            </SectionHeading>
            <InfrastructureRow
              containers={infrastructure}
              deployment={deployment}
              updates={updates}
              onRestart={restart}
            />
          </div>
        )}

        {status && <AdminCard status={status} />}
      </div>
    </Page>
  );
};

export const DashboardScreen = () => {
  const { id } = useParams<{ id: string }>();
  const { byId, loading } = useRegistry();

  const deployment = id ? byId(id) : undefined;

  if (loading) return null;

  // Two dashboards, because the two deployments have nothing in common past the folder:
  // an engine has no profile, no services, no channels and no admin account.
  if (deployment?.kind === "engine") {
    return <EngineDashboard deployment={deployment} />;
  }

  return deployment ? (
    <Dashboard deployment={deployment} />
  ) : (
    <Page
      buttons={
        <Button asChild>
          <Link to="/">Home</Link>
        </Button>
      }
    >
      <div className="my-7">
        <div className="font-light text-4xl">Unknown deployment</div>
        <div className="text-muted-foreground mt-2">
          Konstruktor does not know a deployment with this id any more.
        </div>
      </div>
    </Page>
  );
};
