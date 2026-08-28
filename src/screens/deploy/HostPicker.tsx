import { Badge } from "../../components/ui/badge";
import { Card } from "../../components/ui/card";
import { cn } from "../../utils";
import type { HostCandidate } from "../../api";

/** What a candidate's classification means, in words. */
const describeHostKind = (kind: HostCandidate["kind"]): string => {
  switch (kind) {
    case "public":
      return "reachable from outside this network";
    case "private":
      return "reachable on the local network";
    case "hostname":
      return "a name this machine resolves to";
  }
};

/**
 * The addresses this hub will claim to be reachable at. They become the aliases on
 * every service instance, so a client that cannot resolve any of them cannot reach the
 * hub at all — which is why they are chosen explicitly rather than guessed.
 */
export const HostPicker = ({
  candidates,
  selected,
  onToggle,
  loading,
}: {
  candidates: HostCandidate[];
  selected: Set<string>;
  onToggle: (value: string) => void;
  loading?: boolean;
}) => (
  <div className="flex flex-col gap-2 max-w-2xl">
    {loading && (
      <div className="text-sm text-muted-foreground">Looking at this machine…</div>
    )}
    {!loading && candidates.length === 0 && (
      <div className="text-sm text-muted-foreground">
        No usable addresses were found on this machine.
      </div>
    )}
    {candidates.map((candidate) => (
      <Card
        key={candidate.value}
        onClick={() => onToggle(candidate.value)}
        className={cn(
          "gap-0 py-3 px-4 cursor-pointer border transition-colors",
          selected.has(candidate.value)
            ? "border-primary bg-primary/5"
            : "border-border opacity-70"
        )}
      >
        <div className="flex flex-row items-center gap-3">
          <input type="checkbox" checked={selected.has(candidate.value)} readOnly />
          <div className="flex-grow min-w-0">
            <div className="font-medium truncate">{candidate.value}</div>
            <div className="text-xs text-muted-foreground">
              {describeHostKind(candidate.kind)} · {candidate.interface}
            </div>
          </div>
          <Badge variant="outline" className="font-normal text-[10px]">
            {candidate.kind}
          </Badge>
        </div>
      </Card>
    ))}
  </div>
);
