import { Download } from "lucide-react";

import { CommandButton } from "../../CommandButton";
import { Badge } from "../../components/ui/badge";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../components/ui/card";
import { formatDate, updateSummary, type ServiceUpdate } from "./lifecycle";
import { PENDING_BADGE } from "./tone";

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
export const UpdatesCard = ({
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
