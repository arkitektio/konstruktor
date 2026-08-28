import { fetch } from "@tauri-apps/plugin-http";
import { open } from "@tauri-apps/plugin-shell";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { TbReload } from "react-icons/tb";
import {
  Boxes,
  Check,
  CircleAlert,
  Database,
  Download,
  ExternalLink,
  FolderOpen,
  GitBranch,
  KeyRound,
  RefreshCw,
  ScrollText,
  ShieldCheck,
} from "lucide-react";

import {
  CommandButton,
  DangerousButton,
  DangerousCommandButton,
} from "../CommandButton";
import { AppMenu } from "../components/AppMenu";
import { Alert } from "../components/ui/alert";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../components/ui/card";
import { Page } from "../layout/Page";
import { PageHeader, SectionHeading } from "../layout/PageHeader";
import { ResponsiveGrid } from "../layout/ResponsiveGrid";
import { cn } from "../utils";

import * as api from "../api";
import { useRegistry } from "../registry/registry-context";
import type {
  Container,
  DeploymentRecord,
  HubStatus,
  ImageState,
  ServiceView,
} from "../api";
import { CheckoutsCard, useCheckouts } from "./dashboard/CheckoutsCard";
import {
  formatDate,
  runSummary,
  serviceUpdates,
  stages,
  updateSummary,
  RUN_STATE_LABEL,
  type RunState,
  type ServiceUpdate,
  type Stage,
} from "./dashboard/lifecycle";

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
 */

/**
 * Status reads from `--success` / `--warning` / `--destructive`, never from a raw
 * `green-500`.
 *
 * The brand hue is a knob the user turns (see `lib/brand.ts`), so anything hard-coded to
 * a Tailwind palette colour drifts out of the theme the moment they do. Status colours
 * stay categorical — green is fine, amber is waiting, red is wrong, whatever the brand
 * is set to — but they are categorical *tokens*, tuned once per theme in `globals.css`.
 */
const TONE = {
  ok: "border-success/50 bg-success/10 text-success",
  waiting: "border-warning/60 bg-warning/10 text-warning",
  bad: "border-destructive/50 bg-destructive/10 text-destructive",
  idle: "border-border bg-muted text-muted-foreground",
} as const;

const containerColor = (container: Container) => {
  if (container.state === "running") return "border-success/50";
  if (container.state === "exited") return "border-destructive/50";
  return "border-muted-foreground/30";
};

const serviceColor = (containers: Container[]) => {
  if (containers.length === 0) return "bg-muted border-muted-foreground/30";
  if (containers.every((c) => c.state === "running"))
    return "bg-success/10 border-success/50";
  if (containers.some((c) => c.state === "running"))
    return "bg-warning/10 border-warning/60";
  return "bg-destructive/10 border-destructive/50";
};

/**
 * "Something is waiting for you" is not "something is broken".
 *
 * Amber, never `destructive`: a hub with a pulled-but-unapplied image is working fine,
 * and the red badge that says otherwise is the one thing a user sees first.
 */
const PENDING_BADGE = TONE.waiting;

const RUN_STATE_DOT: Record<RunState, string> = {
  running: "bg-success",
  partial: "bg-warning",
  stopped: "bg-destructive",
  never: "bg-muted-foreground/40",
};

/**
 * Polls a service's health endpoint through the gateway.
 *
 * Through the HTTP plugin rather than the webview's `fetch`: the services send no CORS
 * headers for the webview's origin, so a browser request would fail on every healthy
 * service. The plugin's allow-list lives in `capabilities/migrated.json`.
 *
 * The whole URL is built here rather than by the caller. It used to be assembled at the
 * call site and handed over already interpolated, which made the `if (!url)` guard below
 * useless — a service without a URL produced the truthy string `"undefined/health/"` and
 * was polled anyway, forever reporting itself unhealthy.
 */
const useHealth = (base: string | undefined) => {
  const [healthy, setHealthy] = useState<boolean | undefined>(undefined);

  useEffect(() => {
    if (!base) return;
    let cancelled = false;

    const check = async () => {
      try {
        const response = await fetch(`${base}/health/?format=json`, {
          method: "GET",
        });
        if (!cancelled) setHealthy(response.ok);
      } catch {
        if (!cancelled) setHealthy(false);
      }
    };

    check();
    const timer = setInterval(check, 5000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [base]);

  return healthy;
};

const HealthDot = ({ url }: { url: string | undefined }) => {
  const healthy = useHealth(url);
  return (
    <div
      title={
        healthy === undefined
          ? "Not reachable yet"
          : healthy
            ? "Healthy"
            : "Not responding"
      }
      className={cn(
        "h-2 w-2 rounded-full",
        healthy === undefined
          ? "bg-muted-foreground/40"
          : healthy
            ? "bg-success"
            : "bg-destructive"
      )}
    />
  );
};

// --- the lifecycle rail -----------------------------------------------------

const STAGE_TONE: Record<Stage["state"], string> = {
  done: TONE.ok,
  attention: TONE.waiting,
  waiting: TONE.idle,
};

/**
 * The four stages, side by side, each showing whether it happened and the one fact that
 * says when. A stage nobody has reached yet is drawn muted rather than hidden: the point
 * of the rail is that the missing step is as visible as the finished ones.
 */
const LifecycleRail = ({ stages: list }: { stages: Stage[] }) => (
  <div className="grid gap-2 grid-cols-1 sm:grid-cols-2 lg:grid-cols-4">
    {list.map((stage, index) => (
      <div
        key={stage.key}
        className={cn(
          "rounded-lg border px-3 py-2.5 min-w-0",
          STAGE_TONE[stage.state]
        )}
      >
        <div className="flex items-center gap-2">
          <span className="flex size-5 shrink-0 items-center justify-center rounded-full border border-current text-[10px] font-semibold">
            {stage.state === "done" ? (
              <Check className="size-3" />
            ) : stage.state === "attention" ? (
              <CircleAlert className="size-3" />
            ) : (
              index + 1
            )}
          </span>
          <span className="text-sm font-medium">{stage.label}</span>
        </div>
        <div
          className="mt-1 text-xs text-muted-foreground truncate"
          title={stage.detail}
        >
          {stage.detail}
        </div>
      </div>
    ))}
  </div>
);

// --- channel and updates ----------------------------------------------------

/**
 * Which release channel the hub follows.
 *
 * Nothing in the profile names a channel — the channel is the set of image tags, one per
 * service. When they agree there is a single answer; when they do not — which is the case
 * for a stock hub, since kraph ships on `dev` while the rest ship on `next` — it lists
 * what is actually in play, because picking one of them would state something untrue
 * about the other services. Mixed is a fact about the release tags, not a fault.
 */
const ChannelCard = ({
  status,
  updates,
}: {
  status: HubStatus;
  updates: ServiceUpdate[];
}) => {
  const { tag, tags } = status.channel;
  const byService = new Map(updates.map((u) => [u.service, u]));

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-3">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted">
            <GitBranch className="size-4" />
          </span>
          Channel
          <Badge variant="outline" className="font-mono">
            {tag ?? tags.join(" · ")}
          </Badge>
        </CardTitle>
        <CardDescription>
          {tag
            ? `Every service is pinned to the ${tag} tag. Updating images pulls whatever that tag points at now.`
            : `The services follow more than one tag, which is how they ship — no single channel covers this hub.`}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-1">
        {status.services.map((service) => {
          const built = formatDate(byService.get(service.host)?.imageCreated);
          return (
            <div
              key={service.id}
              className="flex items-center justify-between gap-2 text-xs"
            >
              <span className="text-muted-foreground truncate" title={service.image ?? ""}>
                {service.name}
              </span>
              <span className="flex items-center gap-2 shrink-0">
                {built && <span className="text-muted-foreground">built {built}</span>}
                <Badge variant="outline" className="font-mono">
                  {service.tag ?? "untagged"}
                </Badge>
              </span>
            </div>
          );
        })}
      </CardContent>
    </Card>
  );
};

/**
 * What is waiting to be applied.
 *
 * Two things can be pending, and they need different buttons. An image the profile names
 * but the daemon has never fetched needs a pull; an image that *was* pulled since the
 * container started is already on disk and needs the stack recreated to take effect —
 * `compose up` does that, which is why "Start" is the remedy rather than a restart.
 *
 * What this card cannot tell you is whether something newer exists in the registry:
 * answering that means querying the registry, which nothing here does.
 */
const UpdatesCard = ({
  updates,
  onRefresh,
  path,
}: {
  updates: ServiceUpdate[];
  onRefresh: () => void;
  path: string;
}) => {
  const { pulled, missing } = updateSummary(updates);

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-3">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted">
            <Download className="size-4" />
          </span>
          Updates
          {pulled.length > 0 ? (
            <Badge variant="outline" className={PENDING_BADGE}>
              {pulled.length} ready to apply
            </Badge>
          ) : (
            <Badge variant="outline">
              {missing.length > 0 ? `${missing.length} not pulled` : "nothing waiting"}
            </Badge>
          )}
        </CardTitle>
        <CardDescription>
          {pulled.length > 0
            ? "Newer images are already on this machine, but the containers still run the old ones. Applying updates recreates them."
            : "Every container that is running matches the image its tag points at here. Whether something newer has been published upstream is only answered by pulling."}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {pulled.length > 0 && (
          <div className="flex flex-col gap-1">
            <div className="text-xs font-medium">Pulled, not yet running</div>
            {pulled.map((u) => (
              <div key={u.service} className="text-xs text-muted-foreground">
                <span className="font-mono">{u.service}</span> — {u.image}
                {u.imageCreated && ` · built ${formatDate(u.imageCreated)}`}
              </div>
            ))}
          </div>
        )}

        {missing.length > 0 && (
          <div className="flex flex-col gap-1">
            <div className="text-xs font-medium">
              Not on this machine yet
            </div>
            {missing.map((u) => (
              <div key={u.service} className="text-xs text-muted-foreground">
                <span className="font-mono">{u.service}</span> — {u.image}
              </div>
            ))}
          </div>
        )}

        <div className="flex flex-row flex-wrap gap-2">
          <CommandButton
            title="Pull images"
            runningTitle="Pulling…"
            path={path}
            action="pull"
            callback={onRefresh}
          />
          {pulled.length > 0 && (
            <CommandButton
              title="Apply updates"
              runningTitle="Recreating…"
              path={path}
              action="up"
              callback={onRefresh}
            />
          )}
        </div>
      </CardContent>
    </Card>
  );
};

const AdminCard = ({ status }: { status: HubStatus }) => {
  const [revealed, setRevealed] = useState(false);

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-3">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted">
            <KeyRound className="size-4" />
          </span>
          Admin account
        </CardTitle>
        <CardDescription>
          The superuser for each service's own admin panel. It was generated when the
          deployment was created and is stored in the configuration file.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        <div className="grid grid-cols-3 text-sm gap-2">
          <div className="text-muted-foreground">Username</div>
          <div className="col-span-2">{status.admin_user}</div>
          <div className="text-muted-foreground">Password</div>
          <div className="col-span-2 break-all font-mono text-xs">
            {revealed ? status.admin_password : "•".repeat(24)}
          </div>
        </div>
        <div className="flex flex-row gap-2">
          <Button variant="outline" onClick={() => setRevealed(!revealed)}>
            {revealed ? "Hide" : "Reveal"}
          </Button>
          <Button
            variant="outline"
            onClick={() => writeText(status.admin_password)}
          >
            Copy password
          </Button>
        </div>
      </CardContent>
    </Card>
  );
};

const UPDATE_BADGE: Partial<Record<ServiceUpdate["state"], React.ReactNode>> = {
  pulled: (
    <Badge variant="outline" className={cn("gap-1", PENDING_BADGE)}>
      <Download className="size-3" />
      Update ready
    </Badge>
  ),
  missing: <Badge variant="outline">Not pulled</Badge>,
};

const ServiceCard = ({
  service,
  containers,
  deployment,
  update,
  onRestart,
}: {
  service: ServiceView;
  containers: Container[];
  deployment: DeploymentRecord;
  update: ServiceUpdate | undefined;
  onRestart: (id: string) => void;
}) => (
  <Card className={cn("border p-3", serviceColor(containers))}>
    <div className="flex flex-row justify-between items-start gap-2">
      <div className="min-w-0">
        <div className="flex flex-row items-center gap-2">
          <HealthDot url={service.url} />
          <div className="font-bold truncate">{service.name}</div>
        </div>
        <div className="text-xs text-muted-foreground truncate" title={service.image ?? ""}>
          {service.tag ?? "untagged"}
        </div>
      </div>
      <Badge variant="outline">{service.host}</Badge>
    </div>

    {update && UPDATE_BADGE[update.state] && (
      <div className="mt-2">{UPDATE_BADGE[update.state]}</div>
    )}

    <div className="flex flex-col gap-1 mt-3">
      {containers.map((container) => (
        <div
          key={container.id}
          className={cn(
            "border rounded p-2 flex flex-row justify-between items-center gap-2",
            containerColor(container)
          )}
        >
          <div className="text-xs truncate">
            {container.names?.[0]?.replace(/^\//, "") ?? container.id}
          </div>
          <div className="flex flex-row items-center gap-2">
            <div className="text-xs text-muted-foreground">{container.status}</div>
            <Button
              variant="ghost"
              size="sm"
              title="Restart this container"
              onClick={() => container.id && onRestart(container.id)}
            >
              <TbReload />
            </Button>
          </div>
        </div>
      ))}
      {containers.length === 0 && (
        <div className="text-xs text-muted-foreground">Not running</div>
      )}
    </div>

    <div className="flex flex-row gap-2 mt-3">
      {service.url && (
        <Button variant="outline" size="sm" onClick={() => open(service.url)}>
          Open
        </Button>
      )}
      <Button variant="outline" size="sm" asChild>
        <Link to={`/logs/${deployment.id}/service/${service.host}`}>Logs</Link>
      </Button>
    </div>
  </Card>
);

/**
 * The containers the services stand on — Postgres, Redis, MinIO, the gateway.
 *
 * They used to be fetched and then dropped on the floor: the grouping ran over every
 * container, but only the services in the profile were rendered, so a hub whose database
 * had exited looked entirely healthy. They get a row of their own now, and they count
 * towards the "started" stage.
 */
const InfrastructureRow = ({
  containers,
  updates,
  onRestart,
}: {
  containers: Container[];
  updates: ServiceUpdate[];
  onRestart: (id: string) => void;
}) => {
  const byService = new Map(updates.map((u) => [u.service, u]));

  return (
    <div className="flex flex-row flex-wrap gap-2">
      {containers.map((container) => {
        const update = container.service
          ? byService.get(container.service)
          : undefined;
        return (
          <div
            key={container.id}
            className={cn(
              "border rounded-lg px-3 py-2 flex items-center gap-3",
              containerColor(container)
            )}
          >
            <Database className="size-3.5 text-muted-foreground shrink-0" />
            <div className="min-w-0">
              <div className="text-sm font-medium truncate">
                {container.service ?? container.id}
              </div>
              <div className="text-xs text-muted-foreground truncate">
                {container.status ?? container.state}
                {update?.tag ? ` · ${update.tag}` : ""}
              </div>
            </div>
            {update?.state === "pulled" && (
              <Badge variant="outline" className={cn("shrink-0", PENDING_BADGE)}>
                Update ready
              </Badge>
            )}
            <Button
              variant="ghost"
              size="sm"
              title="Restart this container"
              onClick={() => container.id && onRestart(container.id)}
            >
              <TbReload />
            </Button>
          </div>
        );
      })}
    </div>
  );
};

export const Dashboard = ({ deployment }: { deployment: DeploymentRecord }) => {
  const navigate = useNavigate();
  const { forget, refresh } = useRegistry();

  const [status, setStatus] = useState<HubStatus | undefined>();
  const [profileError, setProfileError] = useState<string | undefined>();
  const [containers, setContainers] = useState<Container[]>([]);
  const [images, setImages] = useState<ImageState[]>([]);
  const { checkouts, reload: reloadCheckouts, replace: replaceCheckout } = useCheckouts(deployment.path);
  /** A branch moved since the stack was last brought up, so the code on disk is ahead. */
  const [switched, setSwitched] = useState(false);

  const loadProfile = useCallback(async () => {
    try {
      setStatus(await api.hubStatus(deployment.path));
      setProfileError(undefined);
    } catch (e) {
      setProfileError(e instanceof Error ? e.message : String(e));
    }
  }, [deployment.path]);

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

  const restart = async (id: string) => {
    await api.restartContainer(id);
    await loadContainers();
  };

  return (
    <Page
      menu={
        <AppMenu
          breadcrumb={deployment.name}
          actions={
            <>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => open(deployment.path)}
              >
                <FolderOpen className="size-3.5" />
                Folder
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  void loadProfile();
                  refreshAll();
                }}
              >
                <RefreshCw className="size-3.5" />
                Reload
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => navigate(`/logs/${deployment.id}`)}
              >
                <ScrollText className="size-3.5" />
                Logs
              </Button>
            </>
          }
        />
      }
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
          {deployment.kind === "hub" && (
            <Button variant="outline" asChild>
              <Link to={`/connect/${deployment.id}`}>Authorize</Link>
            </Button>
          )}
          <Button variant="outline" asChild>
            <Link to="/">Home</Link>
          </Button>
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
            url && (
              <Button variant="outline" size="sm" onClick={() => open(url)}>
                <ExternalLink className="size-3.5" />
                Open {url}
              </Button>
            )
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
          <UpdatesCard
            updates={updates}
            onRefresh={refreshAll}
            path={deployment.path}
          />
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
              updates={updates}
              onRestart={restart}
            />
          </div>
        )}

        {status && <AdminCard status={status} />}

        <div>
          <SectionHeading>Danger zone</SectionHeading>
          <div className="flex flex-row flex-wrap gap-2">
            <DangerousCommandButton
              title="Remove containers"
              confirmTitle="Remove the containers?"
              confirmDescription="Stops and removes the containers and networks. The database and object storage survive."
              path={deployment.path}
              action="down"
              callback={refreshAll}
            />
            <DangerousCommandButton
              title="Delete all data"
              confirmTitle="Delete the data?"
              confirmDescription="Removes the containers AND the volumes: the database and everything stored in this deployment is gone for good."
              path={deployment.path}
              action="down-volumes"
              callback={refreshAll}
            />
            <DangerousButton
              title="Forget deployment"
              confirmTitle="Forget this deployment?"
              confirmDescription="Konstruktor stops listing it. The folder, its configuration and its data are left untouched on disk."
              callback={async () => {
                await forget(deployment.id);
                await refresh();
                navigate("/");
              }}
            />
          </div>
        </div>
      </div>
    </Page>
  );
};

export const DashboardScreen = () => {
  const { id } = useParams<{ id: string }>();
  const { byId, loading } = useRegistry();

  const deployment = id ? byId(id) : undefined;

  if (loading) return null;

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
