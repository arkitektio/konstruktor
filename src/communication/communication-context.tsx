import React, { useContext } from "react";

import type { DockerProbe } from "../api";

export type { DockerProbe };

/**
 * Docker, reduced to the one thing the UI has to decide: what to tell the user next.
 *
 * The three failures are kept apart because their remedies are different — a missing
 * binary is a download, a missing compose plugin is a newer Docker, and a silent daemon
 * is "start Docker Desktop". Collapsing them into "not ok" would send two thirds of the
 * users to the wrong place.
 */
export type DockerState =
  | "checking"
  | "ready"
  | "no-daemon"
  | "no-compose"
  | "missing";

export { dockerState } from "../api";

export interface CommunicationContextType {
  /** The last completed probe, or null while the first one is still running. */
  probe: DockerProbe | null;
  state: DockerState;
  /** A probe is in flight. The previous answer stays visible while it is. */
  checking: boolean;
  /** Run the probe again — what the "Check again" button calls. */
  recheck: () => Promise<DockerProbe>;
}

export const CommunicationContext =
  React.createContext<CommunicationContextType>({
    probe: null,
    state: "checking",
    checking: true,
    recheck: null as unknown as any,
  });

export const useCommunication = () => useContext(CommunicationContext);
