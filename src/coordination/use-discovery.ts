import { useEffect, useState } from "react";
import * as api from "../api";
import type { WellKnownFakts } from "../api";

export type Discovery =
  | { state: "idle" }
  | { state: "looking" }
  | { state: "found"; wellKnown: WellKnownFakts }
  | { state: "failed"; message: string };

/**
 * Looks a coordination server up while the user is still typing its address.
 *
 * Feedback only: it says what is at the other end so somebody can tell go.arkitekt.live
 * from a typo, and it is never what decides whether the wizard may continue. Gating on a
 * network call would leave a user with no connection stuck on a step whose real check —
 * the authorization itself — happens later anyway.
 */
export const useDiscovery = (server: string, delay = 500): Discovery => {
  const [discovery, setDiscovery] = useState<Discovery>({ state: "idle" });

  useEffect(() => {
    const trimmed = server.trim();
    if (trimmed.length === 0) {
      setDiscovery({ state: "idle" });
      return;
    }

    // No abort to hand the core, so a stale answer is discarded on arrival instead.
    let cancelled = false;
    const timer = setTimeout(() => {
      setDiscovery({ state: "looking" });
      api
        .discoverServer(trimmed)
        .then((wellKnown) => {
          if (!cancelled) setDiscovery({ state: "found", wellKnown });
        })
        .catch((error: unknown) => {
          if (cancelled) return;
          setDiscovery({
            state: "failed",
            // The core's errors already say what went wrong and where.
            message: typeof error === "string" ? error : `Could not reach ${trimmed}.`,
          });
        });
    }, delay);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [server, delay]);

  return discovery;
};
