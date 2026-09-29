import { ChevronDown, GitBranch, Settings2 } from "lucide-react";
import type { ReactNode } from "react";

import type { ServiceMeta } from "../../api";
import { Card } from "../../components/ui/card";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "../../components/ui/collapsible";
import { cn } from "../../utils";

/**
 * The service list's parts, shared by the wizard's services step and the dashboard's
 * "Manage services" dialog: one row per service, and the list split into the ordinary
 * services and the collapsed "Experimental" section. Plain props, no form — each caller
 * decides what a click means.
 */

/**
 * One service in the list. Being in the hub is said by the highlight alone — a checkbox
 * next to a card that already changes colour was two controls for one bit, and the tick
 * drew the eye to the wrong thing.
 */
export const ServiceRow = ({
  service,
  on,
  active = false,
  fromSource = false,
  note,
  onClick,
  onGear,
}: {
  service: ServiceMeta;
  on: boolean;
  active?: boolean;
  /** Running from a checkout, which is worth seeing without opening the gear. */
  fromSource?: boolean;
  /** A word on the right of the name — "adding", "removing". */
  note?: string;
  onClick: () => void;
  /** The settings gear. Left out, the row has none. */
  onGear?: () => void;
}) => (
  <Card
    onClick={onClick}
    className={cn(
      "gap-0 py-2.5 px-3 border cursor-pointer transition-colors",
      // In the hub, or not: the highlight is the whole statement.
      on
        ? "border-primary bg-primary/5 font-medium"
        : "border-border text-muted-foreground",
      // Being read about is a different thing from being in, and has to be legible on
      // top of either — hence a ring rather than another shade of the same colour.
      active && "ring-1 ring-foreground/20"
    )}
  >
    <div className="flex items-center gap-2">
      <span className="truncate">{service.name}</span>
      {fromSource && (
        <GitBranch className="size-3.5 shrink-0 text-primary" aria-label="from source" />
      )}
      {!service.emitted && (
        <span className="text-[10px] uppercase tracking-wide text-muted-foreground">
          soon
        </span>
      )}
      {service.experimental && <ExperimentalBadge />}
      {note && (
        <span className="ml-auto text-[10px] uppercase tracking-wide text-primary">
          {note}
        </span>
      )}
      {service.emitted && onGear && (
        <button
          type="button"
          aria-label={`${service.name} settings`}
          // Not the card's click: the gear is for the settings of a service, which is a
          // different question from whether the hub runs it at all.
          onClick={(event) => {
            event.stopPropagation();
            onGear();
          }}
          className={cn(
            "-mr-1 p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted transition-colors",
            !note && "ml-auto"
          )}
        >
          <Settings2 className="size-3.5" />
        </button>
      )}
    </div>
    {/* Listed apart and unfamiliar, so the one-liner comes along with the name. */}
    {service.experimental && (
      <div className="text-xs text-muted-foreground font-normal mt-0.5">
        {service.description}
      </div>
    )}
  </Card>
);

export const ExperimentalBadge = () => (
  <span className="text-[10px] uppercase tracking-wide text-muted-foreground border border-border rounded px-1 py-px">
    experimental
  </span>
);

/**
 * The list: the ordinary services, then the experimental ones under a disclosure —
 * offered to whoever looks, never in the way of whoever does not.
 */
export const ServiceSections = ({
  services,
  row,
  experimentalOpen,
  onExperimentalOpenChange,
}: {
  services: ServiceMeta[];
  row: (service: ServiceMeta) => ReactNode;
  experimentalOpen: boolean;
  onExperimentalOpenChange: (open: boolean) => void;
}) => {
  const stable = services.filter((service) => !service.experimental);
  const experimental = services.filter((service) => service.experimental);

  return (
    <div className="flex flex-col gap-1.5">
      {stable.map(row)}

      {experimental.length > 0 && (
        <Collapsible
          open={experimentalOpen}
          onOpenChange={onExperimentalOpenChange}
          className="mt-2"
        >
          <CollapsibleTrigger asChild>
            <button
              type="button"
              className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
            >
              <ChevronDown
                className={cn(
                  "size-3.5 transition-transform",
                  experimentalOpen && "rotate-180"
                )}
              />
              Experimental ({experimental.length})
            </button>
          </CollapsibleTrigger>
          <CollapsibleContent className="pt-1.5 flex flex-col gap-1.5">
            {experimental.map(row)}
          </CollapsibleContent>
        </Collapsible>
      )}
    </div>
  );
};
