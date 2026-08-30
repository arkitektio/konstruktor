import { open } from "@tauri-apps/plugin-shell";
import {
  CloudDownload,
  Download,
  ExternalLink,
  MoreHorizontal,
  RotateCw,
  ScrollText,
  UserPlus,
} from "lucide-react";
import { useState } from "react";
import { Link } from "react-router-dom";

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
}) => {
  const [admin, setAdmin] = useState(false);
  // An account is made by exec-ing into the container, so there has to be one running.
  // `state` is compose's own word for it — `status` is the human line ("Up 3 minutes").
  const running = containers.some((container) => container.state === "running");
  const built = formatDate(update?.imageCreated);

  const line = statusLine(containers);

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
        {update?.state === "pulled" ? (
          <Badge
            variant="outline"
            className={cn("gap-1 h-5 px-1.5 text-[10px] font-normal", PENDING_BADGE)}
            title="A newer image is on disk; recreate the stack to run it."
          >
            <Download className="size-3" />
            update ready
          </Badge>
        ) : upstream?.state === "newer" ? (
          <Badge
            variant="outline"
            className="gap-1 h-5 px-1.5 text-[10px] font-normal"
            title="The registry has a newer image for this tag. Pull images to fetch it."
          >
            <CloudDownload className="size-3" />
            update
          </Badge>
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
        <span className="truncate" title={line.title}>
          {line.text}
        </span>
      </div>

      <SuperuserDialog
        open={admin}
        onOpenChange={setAdmin}
        path={deployment.path}
        service={service.host}
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
