import { fetch } from "@tauri-apps/plugin-http";
import { useEffect, useState } from "react";

import { cn } from "../../utils";

/**
 * Polls a service's health endpoint through the gateway.
 *
 * Through the HTTP plugin rather than the webview's `fetch`: the services send no CORS
 * headers for the webview's origin, so a browser request would fail on every healthy
 * service. The plugin's allow-list lives in `capabilities/migrated.json`.
 *
 * The whole URL is built here rather than by the caller. It used to be assembled at the
 * call site and handed over already interpolated, which made the `if (!url)` guard below
 * useless — a service without a URL produced the truthy string `"undefined/health/"` and
 * was polled anyway, forever reporting itself unhealthy.
 */
const useHealth = (base: string | undefined) => {
  const [healthy, setHealthy] = useState<boolean | undefined>(undefined);

  useEffect(() => {
    if (!base) return;
    let cancelled = false;

    const check = async () => {
      try {
        const response = await fetch(`${base}/health/?format=json`, {
          method: "GET",
        });
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
  }, [base]);

  return healthy;
};

export const HealthDot = ({ url }: { url: string | undefined }) => {
  const healthy = useHealth(url);
  return (
    <div
      title={
        healthy === undefined
          ? "Not reachable yet"
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
