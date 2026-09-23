import { Globe, Loader2, RefreshCw, Waypoints } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { cn } from "../../utils";
import * as api from "../../api";
import type { AliasProbe } from "../../api";

/** Often enough to notice a gateway that lost a service, rarely enough not to matter. */
const POLL_MS = 30_000;

/**
 * Every service, through every address this hub advertises.
 *
 * The service cards' dots ask through `localhost`, which proves the services work but not
 * that the addresses the coordination server hands out lead to them. This asks the same
 * health path on each advertised alias — a gateway that lost its way to one service shows
 * up here as a red dot on every address at once.
 *
 * Only from this machine: an address it cannot reach is listed as such, greyed, and not
 * counted against the hub.
 */
export const useGatewayCheck = (path: string, stackUp: boolean) => {
  const [aliases, setAliases] = useState<AliasProbe[] | undefined>();
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | undefined>();

  const run = useCallback(async () => {
    setChecking(true);
    try {
      setAliases(await api.gatewayCheck(path));
      setError(undefined);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setChecking(false);
    }
  }, [path]);

  useEffect(() => {
    if (!stackUp) {
      setAliases(undefined);
      return;
    }
    run();
    const timer = setInterval(run, POLL_MS);
    return () => clearInterval(timer);
  }, [stackUp, run]);

  return { aliases, checking, error, run };
};

/** How many reachable addresses have a service that does not answer. */
export const brokenAliases = (aliases: AliasProbe[]) =>
  aliases.filter((a) => a.reachable && a.services.some((s) => !s.healthy)).length;

export const GatewayCheck = ({
  aliases,
  checking,
  error,
  onCheck,
}: {
  aliases: AliasProbe[] | undefined;
  checking: boolean;
  error: string | undefined;
  onCheck: () => void;
}) => {
  if (error) {
    return <div className="text-sm text-destructive">{error}</div>;
  }
  if (aliases === undefined) {
    return (
      <div className="text-sm text-muted-foreground flex items-center gap-2">
        <Loader2 className="size-3.5 animate-spin" /> Asking every address…
      </div>
    );
  }
  if (aliases.length === 0) {
    return (
      <div className="text-sm text-muted-foreground">
        This hub advertises no address yet. Authorize it, and the addresses it is handed out
        at are checked here.
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      {aliases.map((alias) => (
        <div
          key={`${alias.host}:${alias.port}`}
          className={cn(
            "border rounded-md px-3 py-2 flex flex-col gap-1.5",
            !alias.reachable && "opacity-60"
          )}
        >
          <div className="flex items-center gap-2 min-w-0">
            {alias.kind === "mesh" ? (
              <Waypoints className="size-3.5 text-muted-foreground shrink-0" />
            ) : (
              <Globe className="size-3.5 text-muted-foreground shrink-0" />
            )}
            <code className="text-xs truncate">
              {alias.host}:{alias.port}
            </code>
            <Badge variant="outline" className="shrink-0 font-normal text-[10px]">
              {alias.kind}
            </Badge>
            {!alias.reachable && (
              <span className="text-[11px] text-muted-foreground truncate">
                {alias.detail ?? "not reachable from this machine"}
              </span>
            )}
          </div>
          {alias.reachable && (
            <div className="flex flex-row flex-wrap gap-x-3 gap-y-1">
              {alias.services.map((service) => (
                <span
                  key={service.service}
                  className="flex items-center gap-1.5 text-xs"
                  title={service.healthy ? service.url : `${service.url} — ${service.detail}`}
                >
                  <span
                    className={cn(
                      "h-2 w-2 rounded-full",
                      service.healthy ? "bg-success" : "bg-destructive"
                    )}
                  />
                  {service.service}
                  {!service.healthy && service.detail && (
                    <span className="text-destructive">{service.detail}</span>
                  )}
                </span>
              ))}
            </div>
          )}
        </div>
      ))}
      <div>
        <Button
          variant="ghost"
          size="xs"
          className="text-muted-foreground"
          disabled={checking}
          onClick={onCheck}
        >
          <RefreshCw className={cn("size-3.5", checking && "animate-spin")} />
          {checking ? "Checking…" : "Check again"}
        </Button>
      </div>
    </div>
  );
};
