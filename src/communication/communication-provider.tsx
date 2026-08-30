import { getCurrentWindow } from "@tauri-apps/api/window";
import * as api from "../api";
import React, { useCallback, useEffect, useRef, useState } from "react";
import {
  CommunicationContext,
  DockerProbe,
  GitProbe,
  dockerState,
} from "./communication-context";

export type ICommunicationProviderProps = {
  children: React.ReactNode;
};

/** What we assume when the command itself could not be reached at all. */
const unreachable = (error: unknown): DockerProbe => ({
  cli: false,
  cli_version: null,
  compose: false,
  compose_version: null,
  daemon: false,
  api_version: null,
  memory: null,
  error: error instanceof Error ? error.message : String(error),
  engine: null,
  brand: "unknown",
  platform: "other",
  state: "missing",
  remedies: [],
});

/**
 * How often to look again while the engine is not ready. Starts quick — somebody who
 * just clicked Install or Start is watching — and backs off, because a machine that
 * has no engine at all should not be running `docker --version` every five seconds
 * for the rest of the session.
 */
const POLL_FIRST_MS = 5_000;
const POLL_MAX_MS = 30_000;

const CommunicationProvider: React.FC<ICommunicationProviderProps> = ({
  children,
}) => {
  const [probe, setProbe] = useState<DockerProbe | null>(null);
  const [git, setGit] = useState<GitProbe | null>(null);
  const [checking, setChecking] = useState(true);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  // One probe at a time. The poll, a window focus and a button can all ask at once,
  // and three concurrent `docker info`s answer the same question three times as slowly.
  const inFlight = useRef<Promise<DockerProbe> | null>(null);

  const recheck = useCallback(async () => {
    if (inFlight.current) return inFlight.current;
    const run = (async () => {
      setChecking(true);
      let result: DockerProbe;
      try {
        result = await api.probeDocker();
      } catch (error) {
        result = unreachable(error);
      }
    // Git rides along with the Docker recheck rather than having a button of its own:
    // the reason somebody presses "Check again" is that they just installed something,
    // and there is no sense in making them find a second button for the other tool.
    try {
      const found = await api.probeGit();
      if (mounted.current) setGit(found);
    } catch {
      if (mounted.current) setGit({ cli: false, cli_version: null });
    }

      if (mounted.current) {
        setProbe(result);
        setChecking(false);
      }
      return result;
    })();
    inFlight.current = run;
    try {
      return await run;
    } finally {
      inFlight.current = null;
    }
  }, []);

  // The very first probe runs as the app starts, so by the time anybody opens the
  // wizard the answer is usually already there. The wizard re-runs it anyway — a user
  // who leaves to install Docker comes back to a stale one.
  useEffect(() => {
    recheck();
  }, [recheck]);

  const state = dockerState(probe);
  const ready = state === "ready";

  // While the engine is not ready, keep looking. The expected path through any failure
  // is "install or start something, then wait" — and the waiting should not need a
  // button. Backs off per state, and starts over quickly whenever the state changes,
  // which is when somebody is most likely to be watching.
  useEffect(() => {
    if (ready || state === "checking") return;
    let delay = POLL_FIRST_MS;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let stopped = false;
    const tick = async () => {
      if (stopped) return;
      await recheck();
      delay = Math.min(delay * 2, POLL_MAX_MS);
      if (!stopped) timer = setTimeout(tick, delay);
    };
    timer = setTimeout(tick, delay);
    return () => {
      stopped = true;
      if (timer) clearTimeout(timer);
    };
  }, [ready, state, recheck]);

  // Coming back to the window is the other moment worth a look: the user has been off
  // installing something. Outside the Tauri window there is no window to watch.
  useEffect(() => {
    if (ready) return;
    let unlisten: (() => void) | undefined;
    try {
      void getCurrentWindow()
        .onFocusChanged(({ payload: focused }) => {
          if (focused) void recheck();
        })
        .then((stop) => {
          unlisten = stop;
        })
        .catch(() => {});
    } catch {
      // Not inside Tauri.
    }
    return () => unlisten?.();
  }, [ready, recheck]);

  return (
    <CommunicationContext.Provider
      value={{
        probe,
        state,
        checking,
        recheck,
        git,
      }}
    >
      {children}
    </CommunicationContext.Provider>
  );
};

export { CommunicationProvider };
