import { GitBranch } from "lucide-react";

import { Badge } from "../../components/ui/badge";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../components/ui/card";
import type { HubStatus } from "../../api";
import { formatDate, type ServiceUpdate } from "./lifecycle";

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
export const ChannelCard = ({
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
