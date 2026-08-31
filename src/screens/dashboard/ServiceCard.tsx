import { open } from "@tauri-apps/plugin-shell";
import {
  Bug,
  CloudDownload,
  Download,
  ExternalLink,
  Loader2,
  MoreHorizontal,
  RotateCw,
  ScrollText,
  UserPlus,
} from "lucide-react";
import { useRef, useState } from "react";
import { Link } from "react-router-dom";

import * as api from "../../api";
import { useAlerter } from "../../alerter/alerter-context";
import {
  advance,
  EMPTY_PROGRESS,
  newProgressState,
  type ComposeProgress,
} from "../../compose-progress";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "../../components/ui/dropdown-menu";
import { cn } from "../../utils";
import type { Checkout, Container, DeploymentRecord, ServiceView, UpstreamCheck } from "../../api";
import { BranchPicker } from "./CheckoutsCard";
import { HealthDot } from "./HealthDot";
import { formatDate, type ServiceUpdate } from "./lifecycle";
import { BugReportDialog } from "./BugReportDialog";
import { SuperuserDialog } from "./SuperuserDialog";
import { PENDING_BADGE, serviceEdge } from "./tone";

/**
 * One service, in two lines.
 *
 * The first line says what it is and whether it answers; the second says which image —
 * or, on a dev hub, which branch — it runs, and for how long. Everything you can *do* to
 * it sits behind the `…` menu, because five services with three buttons each was fifteen
 * buttons on a page that is meant to be read, not pressed.
 *
 * Colour is a verdict about the *stack*, not the container: a service that is down while
 * the stack is up is red, but when nothing is running there is nothing wrong with any
 * one service, and the tile is simply grey.
 */
export const ServiceCard = ({
  service,
  containers,
  deployment,
  stackUp,
  update,
  upstream,
  checkout,
  onRestart,
  onSwitched,
  onUpdated,
}: {
  service: ServiceView;
  containers: Container[];
  deployment: DeploymentRecord;
  /** Whether the stack as a whole is up — the difference between "stopped" and "off". */
  stackUp: boolean;
  update: ServiceUpdate | undefined;
  /** What the registry said, once the dashboard's background check came back. */
  upstream: UpstreamCheck | undefined;
  /** The source checkout this service runs, on a dev hub. */
  checkout: Checkout | undefined;
  onRestart: (id: string) => void;
  onSwitched: (next: Checkout) => void;
  /** Re-read containers, images and the registry verdict once this service has moved. */
  onUpdated: () => void;
}) => {
  const [admin, setAdmin] = useState(false);
  const [reporting, setReporting] = useState(false);
  // An account is made by exec-ing into the container, so there has to be one running.
  // `state` is compose's own word for it — `status` is the human line ("Up 3 minutes").
  const running = containers.some((container) => container.state === "running");
  const built = formatDate(update?.imageCreated);

  const line = statusLine(containers);
  const { alert } = useAlerter();
  const [updating, setUpdating] = useState(false);
  /**
   * What compose is doing, from its own narration. A pull of a large image is minutes of
   * apparent nothing, so the tile says which layer or container it is on rather than
   * leaving a spinner to imply that something might be happening.
   */
  const [progress, setProgress] = useState<ComposeProgress>(EMPTY_PROGRESS);
  const progressState = useRef(newProgressState());

  /**
   * An image that is already on this machine — `compose pull` moved the tag but nothing
   * recreated the container. Applying it is local work, so it must not be made to wait
   * on a registry that may not answer.
   */
  const ready = update?.state === "pulled";
  /** The registry has something newer, or the image was never fetched at all. */
  const needsPull = upstream?.state === "newer" || update?.state === "missing";
  /*
   * Only while the stack is up. `--no-deps` keeps the update to this one service, which
   * means on a stopped stack it would start exactly one container and leave the hub in
   * the half-up state the page draws as a fault — and the grid already carries a "Start
   * hub" over it there, which is the right remedy.
   */
  const updatable = stackUp && (ready || needsPull) && containers.length > 0;

  /**
   * A pull followed by a recreate narrates two commands into one progress state: the
   * images the first names and the containers the second names land in the same counts,
   * so the fraction would go *backwards* the moment the recreate starts. Two phases get
   * the indeterminate fill and the step text, which is the useful half anyway.
   */
  const fraction = needsPull ? undefined : progress.fraction;

  const applyUpdate = async () => {
    setUpdating(true);
    progressState.current = newProgressState();
    setProgress(EMPTY_PROGRESS);
    try {
      await api.updateService(
        deployment.path,
        service.host,
        Boolean(needsPull),
        (line) =>
          setProgress(advance(progressState.current, line, deployment.project))
      );
      onUpdated();
    } catch (error) {
      alert({
        error: `Could not update ${service.name}`,
        message: typeof error === "string" ? error : String(error),
        subtitle: needsPull
          ? "docker compose could not fetch or recreate this service."
          : "docker compose could not recreate this service.",
      });
    } finally {
      setUpdating(false);
      setProgress(EMPTY_PROGRESS);
    }
  };

  return (
    <div
      className={cn(
        "group rounded-lg border border-l-[3px] bg-card px-3 py-2 min-w-0 transition-opacity",
        serviceEdge(containers, stackUp),
        !stackUp && "opacity-60",
      )}
    >
      <div className="flex items-center gap-2 min-w-0">
        <HealthDot url={stackUp ? service.url : undefined} />
        <div className="text-sm font-medium truncate flex-1" title={service.host}>
          {service.name}
        </div>
        {/*
          The badge does the thing it announces. It used to only say that an update was
          waiting, and left the user to find "Pull images" and "Apply updates" in a card
          further down the page — which updates every service, when the one they were
          looking at is right here.
        */}
        {ready || needsPull ? (
          <Button
            variant="outline"
            size="xs"
            disabled={!updatable || updating}
            onClick={applyUpdate}
            className={cn(
              "relative overflow-hidden h-5 px-1.5 text-[10px] font-normal",
              ready && PENDING_BADGE,
              // Disabled-but-working: the fill and the count have to stay readable.
              updating && "disabled:opacity-100"
            )}
            title={
              !stackUp
                ? "Start the hub first — updating one service on a stopped stack would leave the rest down"
                : ready
                  ? "A newer image is on disk. Recreate this service to run it."
                  : "Fetch the newer image for this tag and recreate this service."
            }
          >
            {/*
              How far compose has got, as a fill creeping in from the left — the same
              language the footer's buttons speak while a stack starts.
            */}
            {updating && (
              <span
                aria-hidden
                className={cn(
                  "absolute inset-y-0 left-0 bg-primary/20 transition-[width] duration-300 ease-out",
                  fraction === undefined && "w-1/5 animate-pulse"
                )}
                style={
                  fraction === undefined
                    ? undefined
                    : { width: `${Math.max(8, Math.round(fraction * 100))}%` }
                }
              />
            )}
            <span className="relative flex items-center gap-1">
              {updating ? (
                <Loader2 className="size-3 animate-spin" />
              ) : ready ? (
                <Download className="size-3" />
              ) : (
                <CloudDownload className="size-3" />
              )}
              {updating
                ? fraction !== undefined && progress.total > 0
                  ? `${progress.done}/${progress.total}`
                  : "updating…"
                : ready
                  ? "apply update"
                  : "update"}
            </span>
          </Button>
        ) : null}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon-xs"
              className="-mr-1 text-muted-foreground opacity-60 group-hover:opacity-100 data-[state=open]:opacity-100"
              title={`Actions for ${service.name}`}
            >
              <MoreHorizontal className="size-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {service.url && (
              <DropdownMenuItem onClick={() => open(service.url)}>
                <ExternalLink className="size-4" />
                Open in browser
              </DropdownMenuItem>
            )}
            <DropdownMenuItem asChild>
              <Link to={`/logs/${deployment.id}/service/${service.host}`}>
                <ScrollText className="size-4" />
                Logs
              </Link>
            </DropdownMenuItem>
            {containers.map((container) => (
              <DropdownMenuItem
                key={container.id}
                onClick={() => container.id && onRestart(container.id)}
              >
                <RotateCw className="size-4" />
                {containers.length > 1
                  ? `Restart ${containerName(container)}`
                  : "Restart"}
              </DropdownMenuItem>
            ))}
            <DropdownMenuSeparator />
            {/*
              To the service's own repository, with its log attached and this hub's
              credentials taken out of it. Under the separator with the other things that
              act on the service rather than navigate to it.
            */}
            <DropdownMenuItem onClick={() => setReporting(true)}>
              <Bug className="size-4" />
              Report a bug
            </DropdownMenuItem>
            <DropdownMenuItem
              disabled={!running}
              onClick={() => setAdmin(true)}
              title={
                running
                  ? undefined
                  : "The service has to be running for an account to be made in it"
              }
            >
              <UserPlus className="size-4" />
              Create admin account
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <div className="mt-0.5 flex items-center gap-1.5 text-xs text-muted-foreground min-w-0">
        {checkout ? (
          <BranchPicker
            path={deployment.path}
            checkout={checkout}
            onSwitched={onSwitched}
            compact
          />
        ) : (
          <span
            className="font-mono truncate"
            title={[service.image, built && `built ${built}`].filter(Boolean).join(" · ")}
          >
            {service.tag ?? "untagged"}
          </span>
        )}
        <span aria-hidden>·</span>
        {/*
          While the update runs the second line reports it, because that is the only
          line on the tile with room for a sentence — and "Up 2 hours" is stale the
          moment the container is recreated anyway.
        */}
        <span
          className="truncate"
          title={updating ? `${progress.done} of ${progress.total}` : line.title}
        >
          {updating ? (progress.step ?? "working…") : line.text}
        </span>
      </div>

      <SuperuserDialog
        open={admin}
        onOpenChange={setAdmin}
        path={deployment.path}
        service={service.host}
      />

      <BugReportDialog
        open={reporting}
        onOpenChange={setReporting}
        path={deployment.path}
        service={service.host}
        name={service.name}
      />
    </div>
  );
};

const containerName = (container: Container) =>
  container.names?.[0]?.replace(/^\//, "") ?? container.id ?? "container";

/** "Up 2 hours", or the one-line truth for a service with several or no containers. */
const statusLine = (containers: Container[]): { text: string; title?: string } => {
  if (containers.length === 0) return { text: "Not running" };
  if (containers.length === 1) {
    const only = containers[0];
    return { text: only.status ?? only.state ?? "Unknown", title: containerName(only) };
  }
  const running = containers.filter((c) => c.state === "running").length;
  return {
    text: `${running}/${containers.length} running`,
    title: containers.map((c) => `${containerName(c)}: ${c.status ?? c.state}`).join("\n"),
  };
};
