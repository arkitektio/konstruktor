import React, { useContext } from "react";

/** The coordination server offered first when a new hub is created. */
export const DEFAULT_COORDINATION_SERVER = "go.arkitekt.live";

export type Settings = {
  theme: string;
  /** Pre-filled on the hub wizard; every hub still stores its own. */
  coordinationServer: string;
  /**
   * Servers the user has picked before, offered alongside the default one. Kept here
   * rather than per-deployment because the point is to not retype an address that a
   * second hub will use as well.
   */
  knownCoordinationServers: string[];
};

export type SettingContext = {
  settings: Settings;
  setSettings: (settings: Settings) => Promise<void>;
};

export const defaultSettings: Settings = {
  theme: "dark",
  coordinationServer: DEFAULT_COORDINATION_SERVER,
  knownCoordinationServers: [],
};

export const SettingsContext = React.createContext<SettingContext>({
  settings: defaultSettings,
  setSettings: async () => undefined,
});

export const useSettings = () => useContext(SettingsContext);
