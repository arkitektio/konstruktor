import { invoke } from "@tauri-apps/api/core";
import { fetch } from "@tauri-apps/plugin-http";
import { open } from "@tauri-apps/plugin-shell";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { TbReload } from "react-icons/tb";
import {
  Boxes,
  ExternalLink,
  FolderOpen,
  RefreshCw,
  ScrollText,
} from "lucide-react";

import { CommandButton, DangerousButton, DangerousCommandButton } from "../CommandButton";
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
import type { DeploymentRecord, HubStatus, ServiceView } from "../api";

/**
 * The management view of a deployment.
 *
 * Everything shown here is derived from the profile YAML in the folder, and from the
 * compose containers running out of the deployment folder. The stack itself carries no
 * Konstruktor-specific labels — it is an ordinary compose project, and can be driven
 * from a terminal in that folder just as well.
 */

export type Container = {
  id: string;
  names: string[];
  status: string;
  labels: { [key: string]: string };
  state: string;
  service?: string;
};

export type ContainerQuery = {
  containers: Container[];
};

const containerColor = (container: Container) => {
  if (container.state === "running") return "border-green-500/50";
  if (container.state === "exited") return "border-destructive/50";
  return "border-muted-foreground/30";
};

const serviceColor = (containers: Container[]) => {
  if (containers.length === 0) return "bg-muted border-muted-foreground/30";
  if (containers.every((c) => c.state === "running"))
    return "bg-green-500/15 border-green-500/50";
  if (containers.some((c) => c.state === "running"))
    return "bg-amber-500/15 border-amber-500/50";
  return "bg-destructive/10 border-destructive/50";
};

/**
 * Polls a service's `/ht` endpoint through the gateway.
 *
 * Through the HTTP plugin rather than the webview's `fetch`: the services send no CORS
 * headers for the webview's origin, so a browser request would fail on every healthy
 * service. The plugin's allow-list lives in `capabilities/migrated.json`.
 */
const useHealth = (url: string | undefined) => {
  const [healthy, setHealthy] = useState<boolean | undefined>(undefined);

  useEffect(() => {
    if (!url) return;
    let cancelled = false;

    const check = async () => {
      try {
        const response = await fetch(`${url}/?format=json`, { method: "GET" });
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
  }, [url]);

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
            ? "bg-green-500"
            : "bg-destructive"
      )}
    />
  );
};

const AdminCard = ({ status }: { status: HubStatus }) => {
  const [revealed, setRevealed] = useState(false);

  return (
    <Card className="max-w-xl">
      <CardHeader>
        <CardTitle>Admin account</CardTitle>
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

const ServiceCard = ({
  service,
  containers,
  deployment,
  onRestart,
}: {
  service: ServiceView;
  containers: Container[];
  deployment: DeploymentRecord;
  onRestart: (id: string) => void;
}) => (
  <Card className={cn("border p-3", serviceColor(containers))}>
    <div className="flex flex-row justify-between items-start">
      <div>
        <div className="flex flex-row items-center gap-2">
          <HealthDot url={`${service.url}/health/`} />
          <div className="font-bold">{service.name}</div>
        </div>
      </div>
      <Badge variant="outline">{service.host}</Badge>
    </div>

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
              onClick={() => onRestart(container.id)}
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

export const Dashboard = ({ deployment }: { deployment: DeploymentRecord }) => {
  const navigate = useNavigate();
  const { forget, refresh } = useRegistry();

  const [status, setStatus] = useState<HubStatus | undefined>();
  const [profileError, setProfileError] = useState<string | undefined>();
  const [containers, setContainers] = useState<Container[]>([]);

  const loadProfile = useCallback(async () => {
    try {
      setStatus(await api.hubStatus(deployment.path));
      setProfileError(undefined);
    } catch (e) {
      setProfileError(e instanceof Error ? e.message : String(e));
    }
  }, [deployment.path, deployment.kind]);

  useEffect(() => {
    loadProfile();
  }, [loadProfile]);

  const loadContainers = useCallback(async () => {
    try {
      const result = await invoke<ContainerQuery>("list_deployment_containers", {
        path: deployment.path,
      });
      setContainers(result.containers);
    } catch (e) {
      // The docker socket is not always reachable (a remote daemon, or no permission);
      // the compose commands still work, so this is not worth shouting about.
      console.error("Could not list containers", e);
      setContainers([]);
    }
  }, [deployment.path]);

  useEffect(() => {
    loadContainers();
    const timer = setInterval(loadContainers, 3000);
    return () => clearInterval(timer);
  }, [loadContainers]);

  const services = status?.services ?? [];

  const byService = useMemo(() => {
    const grouped = new Map<string, Container[]>();
    for (const container of containers) {
      const key = container.service ?? "";
      grouped.set(key, [...(grouped.get(key) ?? []), container]);
    }
    return grouped;
  }, [containers]);

  const url = status?.gateway_url;
  const kind = { label: "Hub" };

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
              <Button variant="ghost" size="sm" onClick={() => loadProfile()}>
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
            title="Start"
            runningTitle="Starting…"
            path={deployment.path}
            action="up"
            callback={loadContainers}
          />
          <DangerousCommandButton
            title="Stop"
            runningTitle="Stopping…"
            confirmTitle="Stop this deployment?"
            confirmDescription="Every service will be shut down. Your data stays where it is."
            path={deployment.path}
            action="stop"
            callback={loadContainers}
          />
          <CommandButton
            title="Update images"
            runningTitle="Pulling…"
            path={deployment.path}
            action="pull"
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
      <div className="flex flex-col gap-6">
        <div>
          <PageHeader
            icon={Boxes}
            title={deployment.name}
            badge={
              <Badge variant="outline" className="font-normal">
                {kind.label}
              </Badge>
            }
            subtitle={
              <span className="truncate block" title={deployment.path}>
                {deployment.path}
              </span>
            }
          />
          {url && (
            <div className="mt-4 flex flex-row items-center gap-3">
              <Button variant="outline" size="sm" onClick={() => open(url)}>
                <ExternalLink className="size-3.5" />
                Open {url}
              </Button>
              {status?.profile.config.coord_server && (
                <span className="text-xs text-muted-foreground">
                  trusting {status.profile.config.coord_server}
                </span>
              )}
            </div>
          )}
        </div>

        {profileError && (
          <Alert variant="destructive" className="max-w-2xl">
            Could not read this deployment's configuration: {profileError}
          </Alert>
        )}

        {status && <AdminCard status={status} />}

        {services.length > 0 && (
          <div>
            <SectionHeading>Services</SectionHeading>
            <ResponsiveGrid>
              {services.map((service) => (
                <ServiceCard
                  key={service.id}
                  service={service}
                  containers={byService.get(service.host) ?? []}
                  deployment={deployment}
                  onRestart={restart}
                />
              ))}
            </ResponsiveGrid>
          </div>
        )}

        <div>
          <SectionHeading>Danger zone</SectionHeading>
          <div className="flex flex-row flex-wrap gap-2">
            <DangerousCommandButton
              title="Remove containers"
              confirmTitle="Remove the containers?"
              confirmDescription="Stops and removes the containers and networks. The database and object storage survive."
              path={deployment.path}
              action="down"
              callback={loadContainers}
            />
            <DangerousCommandButton
              title="Delete all data"
              confirmTitle="Delete the data?"
              confirmDescription="Removes the containers AND the volumes: the database and everything stored in this deployment is gone for good."
              path={deployment.path}
              action="down-volumes"
              callback={loadContainers}
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
