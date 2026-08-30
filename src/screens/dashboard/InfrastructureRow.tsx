import { Database, PlayCircle, ScrollText } from "lucide-react";
import { TbReload } from "react-icons/tb";
import { Link } from "react-router-dom";

import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { cn } from "../../utils";
import type { Container, DeploymentRecord } from "../../api";
import { isInitContainer, type ServiceUpdate } from "./lifecycle";
import { containerColor, PENDING_BADGE } from "./tone";

/**
 * The containers the services stand on — Postgres, Redis, MinIO, the gateway.
 *
 * They used to be fetched and then dropped on the floor: the grouping ran over every
 * container, but only the services in the profile were rendered, so a hub whose database
 * had exited looked entirely healthy. They get a row of their own now, and they count
 * towards the "started" stage.
 */
export const InfrastructureRow = ({
  containers,
  deployment,
  stackUp,
  updates,
  onRestart,
}: {
  containers: Container[];
  /** With the stack off, an exited database is not a fault and is not drawn as one. */
  stackUp: boolean;
  /** Only for the logs link — the log screen is addressed by deployment and service. */
  deployment: DeploymentRecord;
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
              "border rounded-md px-2.5 py-1.5 flex items-center gap-2",
              stackUp ? containerColor(container) : "border-border opacity-60"
            )}
          >
            {isInitContainer(container) ? (
              <PlayCircle className="size-3.5 text-muted-foreground shrink-0" />
            ) : (
              <Database className="size-3.5 text-muted-foreground shrink-0" />
            )}
            <div className="min-w-0">
              <div className="flex items-center gap-1.5">
                <span className="text-xs font-medium truncate">
                  {container.service ?? container.id}
                </span>
                {/*
                  Said on the tile, not just in the colour: this one is *supposed* to be
                  stopped, and "Exited (0)" underneath it otherwise reads as a failure.
                */}
                {isInitContainer(container) && (
                  <Badge variant="outline" className="shrink-0 font-normal text-[10px]">
                    init
                  </Badge>
                )}
              </div>
              <div
                className="text-[11px] text-muted-foreground truncate"
                title={update?.tag ? `${container.service} · ${update.tag}` : undefined}
              >
                {container.status ?? container.state}
              </div>
            </div>
            {update?.state === "pulled" && (
              <Badge
                variant="outline"
                className={cn("shrink-0 h-5 px-1.5 text-[10px] font-normal", PENDING_BADGE)}
              >
                update
              </Badge>
            )}
            {/*
              `compose logs` is addressed by compose service, which is what the label on
              the container gives — a container without one (nothing this stack writes,
              but the field is optional) has no scoped log to link to.
            */}
            {container.service && (
              <Button
                variant="ghost"
                size="icon-xs"
                className="text-muted-foreground"
                title="Logs for this container"
                asChild
              >
                <Link
                  to={`/logs/${deployment.id}/service/${container.service}`}
                >
                  <ScrollText className="size-3.5" />
                </Link>
              </Button>
            )}
            <Button
              variant="ghost"
              size="icon-xs"
              className="text-muted-foreground"
              title={
                isInitContainer(container)
                  ? "Run this init container again"
                  : "Restart this container"
              }
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
