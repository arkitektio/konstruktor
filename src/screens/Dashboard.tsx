import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { Boxes, FlaskConical, GitBranch, Loader2, ShieldCheck } from "lucide-react";

import { CommandButton, DangerousCommandButton } from "../CommandButton";
import { AppMenu } from "../components/AppMenu";
import { Alert } from "../components/ui/alert";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Page } from "../layout/Page";
import { PageHeader, SectionHeading } from "../layout/PageHeader";
import { cn } from "../utils";

import * as api from "../api";
import { useRegistry } from "../registry/registry-context";
import type { Container, DeploymentRecord, HubStatus, ImageState, UpstreamCheck } from "../api";
import { useCheckouts } from "./dashboard/CheckoutsCard";
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
 * started), an updates card appears only when something is waiting, and only then does
 * it list services — with the release channel in that section's heading. The rules behind all of that live in
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
   * What the registries said, asked once when the page opens and again after a pull.
   * `undefined` while the question is out; it is network, and can take a while.
   */
  const [upstream, setUpstream] = useState<UpstreamCheck[] | undefined>();

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

  const checkUpstream = useCallback(async () => {
    if (!isHub) return;
    setUpstream(undefined);
    try {
      setUpstream(await api.checkUpdates(deployment.path));
    } catch (e) {
      // Offline, or a registry that would not say: the tiles simply show no verdict.
      console.error("Could not check the registries for updates", e);
      setUpstream([]);
    }
  }, [deployment.path, isHub]);

  useEffect(() => {
    void checkUpstream();
  }, [checkUpstream]);

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
  const upstreamByService = useMemo(
    () => new Map((upstream ?? []).map((u) => [u.service, u])),
    [upstream]
  );
  const newer = (upstream ?? []).filter((u) => u.state === "newer");
  const checkoutByService = useMemo(
    () => new Map(checkouts.map((c) => [c.service, c])),
    [checkouts]
  );
  const devHub = checkouts.length > 0;
  /** Up or partly up: the difference between a red tile and a grey page. */
  const stackUp = run.state === "running" || run.state === "partial";

  const refreshAll = () => {
    void loadContainers();
    void loadImages();
    void reloadCheckouts();
    // A pull is the one thing that changes the registry's verdict.
    void checkUpstream();
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
        // One action at a time: Start while the stack is off, Stop while it is up. A
        // partly running stack gets both, since either could be what is wanted.
        <>
          {(!stackUp || run.state === "partial") && (
            <CommandButton
              title={run.state === "partial" ? "Recreate" : "Start"}
              runningTitle="Starting…"
              path={deployment.path}
              project={deployment.project}
              action="up"
              callback={refreshAll}
            />
          )}
          {stackUp && (
            <DangerousCommandButton
              title="Stop"
              runningTitle="Stopping…"
              confirmTitle="Stop this deployment?"
              confirmDescription="Every service will be shut down. Your data stays where it is."
              path={deployment.path}
              project={deployment.project}
              action="stop"
              callback={refreshAll}
            />
          )}
        </>
      }
    >
      {/* Kontrol's page rhythm: header, separator, then blocks a fixed gap apart. */}
      <div className="flex flex-col gap-6">
        <PageHeader
          icon={Boxes}
          title={deployment.name}
          badge={
            <span className="flex items-center gap-2">
              <Badge variant="outline" className="font-normal">
                Hub
              </Badge>
              {devHub && (
                <Badge variant="outline" className="font-normal gap-1 border-warning/60 text-warning">
                  <FlaskConical className="size-3" />
                  dev
                </Badge>
              )}
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
              {status?.mesh_hostname && <span>mesh · {status.mesh_hostname}</span>}
            </span>
          }
          actions={
            <>
              {/*
                Where the hub is registered, top right: it is the fact people look for
                first when they have more than one hub, and the gateway URL that used to
                sit here is Orkestrator's business, not this page's.
              */}
              {status?.authorized && (
                <span
                  className="hidden md:flex items-center gap-1.5 text-sm text-muted-foreground"
                  title={`Registered as ${status.identifier ?? "?"} on ${status.profile.config.coord_server}`}
                >
                  <ShieldCheck className="size-3.5 text-success" />
                  <span className="font-medium text-foreground">{status.identifier ?? "authorized"}</span>
                  on {status.profile.config.coord_server}
                </span>
              )}
              <DeploymentMenu
                deployment={deployment}
                status={status}
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

        <LifecycleRail stages={rail} />

        {/*
          Always on a dev hub, never dismissable. The services run whatever is checked
          out under `mounts/`, and a branch switch can migrate the database forward in a
          way no tag ever would — so the hub is a workbench, and the page keeps saying so.
        */}
        {devHub && (
          <Alert className="max-w-3xl text-sm border-warning/60 [&>svg]:text-warning">
            <FlaskConical className="size-4" />
            <span>
              <span className="font-medium">Development hub.</span> The services run from
              source checkouts, and switching branches can change their schemas underneath
              the data. Data integrity cannot be ensured — never use this hub with
              production data.
            </span>
          </Alert>
        )}
        {switched && (
          <Alert className="max-w-2xl text-sm items-center">
            <span className="flex-1">
              The checkout moved, but the containers are still running the code they
              started with. Recreate the stack to pick it up.
            </span>
            <CommandButton
              title="Recreate"
              runningTitle="Recreating…"
              path={deployment.path}
              project={deployment.project}
              action="up"
              callback={refreshAll}
            />
          </Alert>
        )}

        {/*
          The updates card stays outside the profile guard: it needs only the images and
          the folder, and it carries the one button that pulls them. A hub whose profile
          cannot be read is exactly the one you might want to pull images for.
        */}
        <div className="grid gap-4 grid-cols-1 lg:grid-cols-2">
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
            {/*
              The release channel lives in the heading rather than in a card of its own:
              it is one word ("next"), or a short list when the services disagree — which
              is how a stock hub ships, kraph on `dev` and the rest on `next` — and every
              tile below carries its own tag anyway.
            */}
            <SectionHeading
              hint={
                pending.pulled.length > 0
                  ? `${pending.pulled.length} of these are running an image older than the one on disk.`
                  : newer.length > 0
                    ? `${newer.length} of these have a newer image upstream — pull images from the menu to fetch them.`
                    : upstream === undefined
                      ? "Checking the registries for newer images…"
                      : devHub
                        ? "Each service runs the branch shown on its tile. Switching is refused over uncommitted changes."
                        : undefined
              }
            >
              <span className="flex items-center gap-2">
                Services
                {status && (
                  <Badge
                    variant="outline"
                    className="gap-1 font-mono font-normal normal-case tracking-normal"
                    title={
                      status.channel.tag
                        ? `Every service follows the ${status.channel.tag} tag.`
                        : "The services follow more than one tag."
                    }
                  >
                    <GitBranch className="size-3" />
                    {status.channel.tag ?? status.channel.tags.join(" · ")}
                  </Badge>
                )}
                {upstream === undefined && (
                  <Loader2 className="size-3 animate-spin" />
                )}
              </span>
            </SectionHeading>
            <div className="relative">
              <div className="grid gap-2 grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4">
                {services.map((service) => (
                  <ServiceCard
                    key={service.id}
                    service={service}
                    containers={byService.get(service.host) ?? []}
                    deployment={deployment}
                    stackUp={stackUp}
                    update={updatesByService.get(service.host)}
                    upstream={upstreamByService.get(service.host)}
                    checkout={checkoutByService.get(service.host)}
                    onRestart={restart}
                    onSwitched={(next) => {
                      replaceCheckout(next);
                      setSwitched(true);
                    }}
                  />
                ))}
              </div>
              {/*
                The stack is off: the grid stays legible underneath, greyed, and the one
                thing to do sits on top of it. This is the same Start as the footer's.
              */}
              {!stackUp && (
                <div className="absolute inset-0 flex items-center justify-center rounded-lg bg-background/40 backdrop-blur-[1px]">
                  <div className="flex flex-col items-center gap-2 rounded-lg border bg-card px-5 py-4 shadow-sm">
                    <div className="text-sm text-muted-foreground">
                      {run.state === "never" ? "This hub has never been started." : "This hub is stopped."}
                    </div>
                    <CommandButton
                      title="Start hub"
                      runningTitle="Starting…"
                      path={deployment.path}
                      project={deployment.project}
                      action="up"
                      callback={refreshAll}
                    />
                  </div>
                </div>
              )}
            </div>
          </div>
        )}

        {infrastructure.length > 0 && (
          <div>
            <SectionHeading hint="The database, cache, object storage and gateway the services run on.">
              Infrastructure
            </SectionHeading>
            <InfrastructureRow
              containers={infrastructure}
              stackUp={stackUp}
              deployment={deployment}
              updates={updates}
              onRestart={restart}
            />
          </div>
        )}

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
