import { fetch } from "@tauri-apps/plugin-http";
import { useEffect, useState } from "react";

import { cn } from "../../utils";

/**
 * Polls a service's health check.
 *
 * The URL is the core's (`ServiceView.health_url`, from `konstruktor_core::health`), the
 * same one the restore's and the update's checks ask — building it here as well is how
 * the dot and those checks came to ask different hosts once a hub had a domain.
 *
 * Through the HTTP plugin rather than the webview's `fetch`: the services send no CORS
 * headers for the webview's origin, so a browser request would fail on every healthy
 * service. The plugin's allow-list lives in `capabilities/migrated.json`.
 */
const useHealth = (url: string | null | undefined) => {
  const [healthy, setHealthy] = useState<boolean | undefined>(undefined);

  useEffect(() => {
    if (!url) {
      setHealthy(undefined);
      return;
    }
    let cancelled = false;

    const check = async () => {
      try {
        const response = await fetch(url, { method: "GET" });
        if (!cancelled) setHealthy(response.ok);
      } catch {
        if (!cancelled) setHealthy(false);
      }
    };

    check();
    const timer = setInterval(check, 5000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [url]);

  return healthy;
};

/**
 * `url` is `null` for a service this machine cannot ask — a mesh-only hub publishes no
 * port here — and the dot stays grey rather than red.
 */
export const HealthDot = ({ url }: { url: string | null | undefined }) => {
  const healthy = useHealth(url);
  return (
    <div
      title={
        healthy === undefined
          ? url === null
            ? "Not reachable from this machine — see the gateway check"
            : "Not reachable yet"
          : healthy
            ? "Healthy"
            : "Not responding"
      }
      className={cn(
        "h-2 w-2 rounded-full",
        healthy === undefined
          ? "bg-muted-foreground/40"
          : healthy
            ? "bg-success"
            : "bg-destructive"
      )}
    />
  );
};
