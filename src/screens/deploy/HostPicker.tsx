import { ChevronDown, Globe, Laptop, Network, ShieldQuestion } from "lucide-react";
import { useState } from "react";

import { Badge } from "../../components/ui/badge";
import { Card } from "../../components/ui/card";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "../../components/ui/collapsible";
import { cn } from "../../utils";
import type {
  AdvertisedHost,
  HostCandidate,
  HostCategory,
  ProbeResult,
  ReachPreset,
  ReachPresetId,
} from "../../api";

/** A preset, or the state of having strayed from all of them. */
export type ReachChoice = ReachPresetId | "custom";

/** What reachability, if anything, has been established for one address. */
export type Reachability = {
  /** The internet sees this machine as this address. Says nothing about open ports. */
  egress?: boolean;
  /** Something outside connected back. The only fact that justifies `public`. */
  probe?: ProbeResult;
};

const PRESET_ICONS: Record<ReachPresetId, typeof Laptop> = {
  "local-only": Laptop,
  "this-network": Network,
  public: Globe,
};

/**
 * The groups the detail list is broken into, in the order they are shown.
 *
 * Headings are product wording and live here; what each *candidate* is stays in
 * `candidate.summary`, which the core writes — the frontend used to keep a second copy of
 * those sentences and the two had to be kept in step by hand.
 */
const GROUPS: { kind: HostCategory; heading: string }[] = [
  { kind: "loopback", heading: "This machine" },
  { kind: "private", heading: "Local network" },
  { kind: "mesh", heading: "Mesh" },
  { kind: "public", heading: "Public" },
  { kind: "verified-fqdn", heading: "Verified names" },
  { kind: "fqdn", heading: "Names" },
  { kind: "mdns-name", heading: "mDNS names" },
  { kind: "bare-hostname", heading: "Bare names" },
  // Last of the usable groups: a tailnet this hub is not on is reachable by the machines
  // on that tailnet and nobody else, so it is worth offering and never worth assuming.
  { kind: "other-mesh", heading: "Other tailscales" },
];

const SHORT_LABEL: Record<HostCategory, string> = {
  loopback: "loopback",
  private: "private",
  mesh: "mesh",
  "other-mesh": "other tailnet",
  public: "public",
  virtual: "bridge",
  "link-local": "link-local",
  "mdns-name": "mDNS",
  "bare-hostname": "bare name",
  fqdn: "name",
  "verified-fqdn": "verified",
};

/**
 * The two reachability facts, worded so they cannot be mistaken for each other.
 *
 * "The internet sees you here" is cheap and says nothing about whether a port is open;
 * only the probe does. A single green tick covering both would be the easiest way to
 * tell somebody their firewall is fine when nobody has checked.
 */
const ReachabilityBadges = ({
  reachability,
  canProbe,
}: {
  reachability?: Reachability;
  canProbe: boolean;
}) => {
  const probe = reachability?.probe;

  return (
    <div className="flex flex-row flex-wrap items-center gap-1.5 mt-1">
      {reachability?.egress && (
        <Badge variant="outline" className="font-normal text-[10px]">
          the internet sees you at this address
        </Badge>
      )}
      {probe?.result === "reachable" && (
        <Badge className="font-normal text-[10px]">answered from outside</Badge>
      )}
      {probe?.result === "unreachable" && (
        <Badge variant="destructive" className="font-normal text-[10px]">
          nothing answered from outside
        </Badge>
      )}
      {(!probe || probe.result === "not-checked") && !canProbe && (
        <span className="text-[10px] text-muted-foreground inline-flex items-center gap-1">
          <ShieldQuestion className="size-3" />
          cannot check from outside until the hub is running
        </span>
      )}
    </div>
  );
};

const CandidateCard = ({
  candidate,
  selected,
  onToggle,
  reachability,
  canProbe,
}: {
  candidate: HostCandidate;
  selected: boolean;
  onToggle: (value: string) => void;
  reachability?: Reachability;
  canProbe: boolean;
}) => (
  <Card
    onClick={() => onToggle(candidate.value)}
    className={cn(
      "gap-0 py-3 px-4 cursor-pointer border transition-colors",
      selected ? "border-primary bg-primary/5" : "border-border opacity-70"
    )}
  >
    <div className="flex flex-row items-center gap-3">
      <input type="checkbox" checked={selected} readOnly />
      <div className="flex-grow min-w-0">
        <div className="font-medium truncate">{candidate.value}</div>
        <div className="text-xs text-muted-foreground">
          {candidate.summary} · {candidate.interface}
        </div>
        <ReachabilityBadges reachability={reachability} canProbe={canProbe} />
      </div>
      <Badge variant="outline" className="font-normal text-[10px]">
        {SHORT_LABEL[candidate.kind]}
      </Badge>
    </div>
  </Card>
);

/**
 * The addresses this hub will claim to be reachable at.
 *
 * They become the aliases on every service instance, so a client that cannot resolve any
 * of them cannot reach the hub at all — which is why they are chosen explicitly rather
 * than guessed. The presets answer the question most people actually have ("how far
 * should this thing reach"), and the list underneath is there for when the answer is
 * "not quite any of those".
 *
 * The selection is a list of `{host, kind}` rather than a set of strings on purpose: a
 * hub that moved networks has hosts worth keeping that this machine can no longer
 * discover, and carrying the kind is what lets them survive a trip through this screen.
 */
export const HostPicker = ({
  candidates,
  presets,
  selected,
  reach,
  onReachChange,
  onToggle,
  reachability,
  canProbe = false,
  loading,
  error,
}: {
  candidates: HostCandidate[];
  presets: ReachPreset[];
  selected: AdvertisedHost[];
  reach: ReachChoice;
  onReachChange: (preset: ReachPreset) => void;
  onToggle: (value: string) => void;
  /** Per-address, keyed by value. Absent means nobody has looked. */
  reachability?: Record<string, Reachability>;
  /** Whether a probe could succeed at all — false before the stack is up. */
  canProbe?: boolean;
  loading?: boolean;
  error?: string | null;
}) => {
  const [showUnusable, setShowUnusable] = useState(false);

  const isSelected = (value: string) => selected.some((h) => h.host === value);
  const usable = candidates.filter((c) => c.usable);
  const unusable = candidates.filter((c) => !c.usable);

  // Hosts the hub already advertises that this machine cannot find today. Keeping them
  // visible is the only way they can be removed, and dropping them silently is how a
  // re-authorization quietly narrows a hub that had been reachable.
  const missing = selected.filter(
    (host) => !candidates.some((c) => c.value === host.host)
  );

  if (loading) {
    return <div className="text-sm text-muted-foreground">Looking at this machine…</div>;
  }

  return (
    <div className="flex flex-col gap-5 max-w-2xl">
      {error && <div className="text-sm text-destructive">{error}</div>}

      {candidates.length === 0 && !error && (
        <div className="text-sm text-muted-foreground">
          No addresses were found on this machine.
        </div>
      )}

      {presets.length > 0 && (
        <div className="flex flex-col gap-2">
          <div className="flex flex-row items-center gap-2">
            <span className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
              Reach
            </span>
            {reach === "custom" && (
              <Badge variant="outline" className="font-normal text-[10px]">
                custom
              </Badge>
            )}
          </div>
          {presets.map((preset) => {
            const Icon = PRESET_ICONS[preset.id];
            const empty = preset.values.length === 0;
            return (
              <Card
                key={preset.id}
                onClick={() => !empty && onReachChange(preset)}
                className={cn(
                  "gap-0 py-3 px-4 border transition-colors",
                  empty
                    ? "border-border opacity-50 cursor-not-allowed"
                    : "cursor-pointer",
                  reach === preset.id && !empty
                    ? "border-primary bg-primary/5"
                    : "border-border"
                )}
              >
                <div className="flex items-start gap-3">
                  <span
                    className={cn(
                      "mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md border",
                      reach === preset.id && !empty
                        ? "border-primary text-primary"
                        : "border-border text-muted-foreground"
                    )}
                  >
                    <Icon className="size-3.5" />
                  </span>
                  <div className="min-w-0">
                    <div className="font-medium">{preset.label}</div>
                    <div className="text-sm text-muted-foreground mt-0.5">
                      {empty
                        ? "Nothing on this machine answers that description."
                        : preset.description}
                    </div>
                  </div>
                </div>
              </Card>
            );
          })}
        </div>
      )}

      {GROUPS.map(({ kind, heading }) => {
        const group = usable.filter((c) => c.kind === kind);
        if (group.length === 0) return null;
        return (
          <div key={kind} className="flex flex-col gap-2">
            <span className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
              {heading}
            </span>
            {group.map((candidate) => (
              <CandidateCard
                key={candidate.value}
                candidate={candidate}
                selected={isSelected(candidate.value)}
                onToggle={onToggle}
                reachability={reachability?.[candidate.value]}
                canProbe={canProbe}
              />
            ))}
          </div>
        );
      })}

      {missing.length > 0 && (
        <div className="flex flex-col gap-2">
          <span className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
            Advertised before, not found here now
          </span>
          {missing.map((host) => (
            <Card
              key={host.host}
              onClick={() => onToggle(host.host)}
              className="gap-0 py-3 px-4 cursor-pointer border border-primary bg-primary/5"
            >
              <div className="flex flex-row items-center gap-3">
                <input type="checkbox" checked readOnly />
                <div className="flex-grow min-w-0">
                  <div className="font-medium truncate">{host.host}</div>
                  <div className="text-xs text-muted-foreground">
                    still advertised — untick to stop offering it
                  </div>
                </div>
              </div>
            </Card>
          ))}
        </div>
      )}

      {unusable.length > 0 && (
        <Collapsible open={showUnusable} onOpenChange={setShowUnusable}>
          <CollapsibleTrigger asChild>
            <button
              type="button"
              className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
            >
              <ChevronDown
                className={cn("size-3.5 transition-transform", showUnusable && "rotate-180")}
              />
              {showUnusable ? "Hide" : "Show"} {unusable.length} address
              {unusable.length === 1 ? "" : "es"} that are not worth advertising
            </button>
          </CollapsibleTrigger>
          <CollapsibleContent className="pt-3 flex flex-col gap-2">
            {unusable.map((candidate) => (
              <CandidateCard
                key={candidate.value}
                candidate={candidate}
                selected={isSelected(candidate.value)}
                onToggle={onToggle}
                reachability={reachability?.[candidate.value]}
                canProbe={canProbe}
              />
            ))}
          </CollapsibleContent>
        </Collapsible>
      )}
    </div>
  );
};
