import * as api from "../api";
import React, { useCallback, useEffect, useRef, useState } from "react";
import {
  CommunicationContext,
  DockerProbe,
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
});

const CommunicationProvider: React.FC<ICommunicationProviderProps> = ({
  children,
}) => {
  const [probe, setProbe] = useState<DockerProbe | null>(null);
  const [checking, setChecking] = useState(true);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const recheck = useCallback(async () => {
    setChecking(true);
    let result: DockerProbe;
    try {
      result = await api.probeDocker();
    } catch (error) {
      result = unreachable(error);
    }
    if (mounted.current) {
      setProbe(result);
      setChecking(false);
    }
    return result;
  }, []);

  // The very first probe runs as the app starts, so by the time anybody opens the
  // wizard the answer is usually already there. The wizard re-runs it anyway — a user
  // who leaves to install Docker comes back to a stale one.
  useEffect(() => {
    recheck();
  }, [recheck]);

  return (
    <CommunicationContext.Provider
      value={{
        probe,
        state: dockerState(probe),
        checking,
        recheck,
      }}
    >
      {children}
    </CommunicationContext.Provider>
  );
};

export { CommunicationProvider };
