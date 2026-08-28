import { open } from "@tauri-apps/plugin-shell";
import { Download, UserPlus } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router-dom";
import { TbReload } from "react-icons/tb";

import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Card } from "../../components/ui/card";
import { cn } from "../../utils";
import type { Container, DeploymentRecord, ServiceView } from "../../api";
import { HealthDot } from "./HealthDot";
import type { ServiceUpdate } from "./lifecycle";
import { SuperuserDialog } from "./SuperuserDialog";
import { containerColor, PENDING_BADGE, serviceColor } from "./tone";

const UPDATE_BADGE: Partial<Record<ServiceUpdate["state"], React.ReactNode>> = {
  pulled: (
    <Badge variant="outline" className={cn("gap-1", PENDING_BADGE)}>
      <Download className="size-3" />
      Update ready
    </Badge>
  ),
  missing: <Badge variant="outline">Not pulled</Badge>,
};

export const ServiceCard = ({
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
}) => {
  const [admin, setAdmin] = useState(false);
  // An account is made by exec-ing into the container, so there has to be one running.
  // `state` is compose's own word for it — `status` is the human line ("Up 3 minutes").
  const running = containers.some((container) => container.state === "running");

  return (
    <Card className={cn("border p-3", serviceColor(containers))}>
      <div className="flex flex-row justify-between items-start gap-2">
        <div className="min-w-0">
          <div className="flex flex-row items-center gap-2">
            <HealthDot url={service.url} />
            <div className="font-bold truncate">{service.name}</div>
          </div>
          <div
            className="text-xs text-muted-foreground truncate"
            title={service.image ?? ""}
          >
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
              containerColor(container),
            )}
          >
            <div className="text-xs truncate">
              {container.names?.[0]?.replace(/^\//, "") ?? container.id}
            </div>
            <div className="flex flex-row items-center gap-2">
              <div className="text-xs text-muted-foreground">
                {container.status}
              </div>
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
          <Link to={`/logs/${deployment.id}/service/${service.host}`}>
            Logs
          </Link>
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={!running}
          title={
            running
              ? `Create an admin account for ${service.host}`
              : "The service has to be running for an account to be made in it"
          }
          onClick={() => setAdmin(true)}
        >
          <UserPlus className="size-3.5" />
          Admin
        </Button>
      </div>

      <SuperuserDialog
        open={admin}
        onOpenChange={setAdmin}
        path={deployment.path}
        service={service.host}
      />
    </Card>
  );
};
