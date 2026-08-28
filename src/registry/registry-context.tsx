import React, { useContext } from "react";
import type { DeploymentRecord, FolderReport } from "../api";

/**
 * The deployments this machine knows about.
 *
 * The list itself lives in Rust — the same `deployments.json` the `konstruktor` command
 * reads — so a hub created either way shows up in both. This context is only the React
 * side of it: a cached copy and the calls that refresh it.
 */
export type RegistryContextType = {
  deployments: DeploymentRecord[];
  loading: boolean;
  /** Ask the user for a folder, and grant the app access to it. */
  pickFolder: (title?: string) => Promise<string | undefined>;
  /** The folder offered before the user picks one: `~/MyHub`, or the next free one. */
  suggestFolder: () => Promise<string | undefined>;
  /** Whether a deployment can live in a folder, and why not when it cannot. */
  inspectFolder: (path: string) => Promise<FolderReport>;
  forget: (id: string) => Promise<void>;
  byId: (id: string) => DeploymentRecord | undefined;
  refresh: () => Promise<void>;
};

export const RegistryContext = React.createContext<RegistryContextType>({
  deployments: [],
  loading: true,
  pickFolder: async () => undefined,
  suggestFolder: async () => undefined,
  inspectFolder: async () => ({ ok: false, message: "No registry provider mounted" }),
  forget: async () => undefined,
  byId: () => undefined,
  refresh: async () => undefined,
});

export const useRegistry = () => useContext(RegistryContext);
