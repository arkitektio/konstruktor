import {
  CircleCheck,
  Globe,
  Loader2,
  Pencil,
  Star,
  TriangleAlert,
} from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { Badge } from "../components/ui/badge";
import { Card } from "../components/ui/card";
import { Input } from "../components/ui/input";
import {
  DEFAULT_COORDINATION_SERVER,
  useSettings,
} from "../settings/settings-context";
import { cn } from "../utils";
import { useDiscovery } from "./use-discovery";
import { baseUrl } from "../screens/deploy/hub-form";

/** The address, normalised the way the rest of the app compares addresses. */
const same = (a: string, b: string) =>
  baseUrl(a).toLowerCase() === baseUrl(b).toLowerCase();

/**
 * Choosing the coordination server, as a choice rather than a text field.
 *
 * The default is offered as something to click, every server this machine has used
 * before is offered next to it, and "another server" is the last option rather than the
 * only one. Whatever ends up selected is looked up live, so the difference between the
 * real go.arkitekt.live and a near miss is visible before the hub is named after it.
 */
export const CoordinationPicker = ({
  value,
  onChange,
}: {
  value: string;
  onChange: (server: string) => void;
}) => {
  const { settings } = useSettings();

  const offered = useMemo(() => {
    const seen = new Set<string>();
    const list: string[] = [];
    for (const server of [
      DEFAULT_COORDINATION_SERVER,
      ...settings.knownCoordinationServers,
    ]) {
      const key = baseUrl(server).toLowerCase();
      if (server.trim().length === 0 || seen.has(key)) continue;
      seen.add(key);
      list.push(server);
    }
    return list;
  }, [settings.knownCoordinationServers]);

  const isOffered = offered.some((server) => same(server, value));

  // "Custom" stays open once opened, even if what was typed happens to match an offered
  // server — closing the field under the cursor mid-keystroke is worse than a duplicate.
  const [custom, setCustom] = useState(() => value.trim().length > 0 && !isOffered);
  const customInput = useRef<HTMLInputElement>(null);

  const discovery = useDiscovery(value);

  const openCustom = () => {
    // Already open: a second click on the card must not wipe what is being typed.
    if (custom) {
      customInput.current?.focus();
      return;
    }
    setCustom(true);
    onChange("");
    // The input does not exist until this render commits.
    requestAnimationFrame(() => customInput.current?.focus());
  };

  return (
    <div className="flex flex-col gap-2 max-w-xl">
      {offered.map((server) => {
        const selected = !custom && same(server, value);
        return (
          <ServerCard
            key={server}
            selected={selected}
            onSelect={() => {
              setCustom(false);
              onChange(server);
            }}
            icon={server === DEFAULT_COORDINATION_SERVER ? Star : Globe}
            name={server}
            note={
              server === DEFAULT_COORDINATION_SERVER
                ? "The public Arkitekt coordination server"
                : "Used before on this machine"
            }
            badge={server === DEFAULT_COORDINATION_SERVER ? "Recommended" : undefined}
            detail={selected ? <Detail discovery={discovery} /> : null}
          />
        );
      })}

      <ServerCard
        selected={custom}
        onSelect={openCustom}
        icon={Pencil}
        name="Another server"
        note="Your institute's own, or one running on this network"
        detail={
          custom ? (
            <div className="flex flex-col gap-2">
              <Input
                ref={customInput}
                value={value}
                onChange={(event) => onChange(event.target.value)}
                onClick={(event) => event.stopPropagation()}
                placeholder="arkitekt.my-institute.org"
                autoComplete="off"
                spellCheck={false}
                className="h-9"
              />
              <Detail discovery={discovery} />
            </div>
          ) : null
        }
      />
    </div>
  );
};

const ServerCard = ({
  selected,
  onSelect,
  icon: Icon,
  name,
  note,
  badge,
  detail,
}: {
  selected: boolean;
  onSelect: () => void;
  icon: React.ComponentType<{ className?: string }>;
  name: string;
  note: string;
  badge?: string;
  detail?: React.ReactNode;
}) => (
  <Card
    onClick={onSelect}
    className={cn(
      "gap-0 py-3 cursor-pointer border transition-colors",
      selected ? "border-primary bg-primary/5" : "border-border hover:border-border/80"
    )}
  >
    <div className="px-4 flex items-start gap-3">
      <span
        className={cn(
          "mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md border",
          selected ? "border-primary text-primary" : "border-border text-muted-foreground"
        )}
      >
        <Icon className="size-3.5" />
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="font-medium truncate">{name}</span>
          {badge && (
            <Badge variant="outline" className="font-normal text-[10px]">
              {badge}
            </Badge>
          )}
        </div>
        <div className="text-xs text-muted-foreground mt-0.5">{note}</div>
        {detail && <div className="mt-3">{detail}</div>}
      </div>
    </div>
  </Card>
);

/** What answered at that address — the reason to look it up while typing. */
const Detail = ({ discovery }: { discovery: ReturnType<typeof useDiscovery> }) => {
  if (discovery.state === "idle") return null;

  if (discovery.state === "looking") {
    return (
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <Loader2 className="size-3.5 animate-spin" />
        Looking it up…
      </div>
    );
  }

  if (discovery.state === "failed") {
    return (
      <div className="flex items-start gap-2 text-xs text-destructive">
        <TriangleAlert className="size-3.5 shrink-0 mt-0.5" />
        <span>{discovery.message}</span>
      </div>
    );
  }

  const { name, description, version } = discovery.wellKnown;
  return (
    <div className="flex items-start gap-2 text-xs">
      <CircleCheck className="size-3.5 shrink-0 mt-0.5 text-primary" />
      <div className="min-w-0">
        <div className="font-medium">
          {name ?? "An Arkitekt coordination server"}
          {version ? (
            <span className="text-muted-foreground font-normal"> · {version}</span>
          ) : null}
        </div>
        {description && (
          <div className="text-muted-foreground mt-0.5">{description}</div>
        )}
      </div>
    </div>
  );
};
