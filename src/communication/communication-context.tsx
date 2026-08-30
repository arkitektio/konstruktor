import React, { useContext } from "react";

import type { DockerProbe, GitProbe } from "../api";

export type { DockerProbe, GitProbe };

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
  | "too-old"
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
  /**
   * Git, which is optional. Kept beside the Docker probe rather than folded into it:
   * `DockerState` is a verdict about whether a deployment can be created at all, and git
   * only decides whether the dev-hub option is offered. `null` while the first probe runs.
   */
  git: GitProbe | null;
}

export const CommunicationContext =
  React.createContext<CommunicationContextType>({
    probe: null,
    state: "checking",
    checking: true,
    recheck: null as unknown as any,
    git: null,
  });

export const useCommunication = () => useContext(CommunicationContext);
