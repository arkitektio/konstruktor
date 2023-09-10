import React, { useContext } from "react";
import { Channel, SetupValues } from "../repo/repo-context";

export type App = {
  path: string;
  name: string;
};

export type StorageContextType = {
  apps: App[];
  installApp: (
    name: string,
    channel: Channel,
    app: Partial<SetupValues>
  ) => Promise<string>;
  deleteApp: (name: string) => Promise<void>;
};

export const StorageContext = React.createContext<StorageContextType>({
  apps: [],
  installApp: async (app) => "fake",
  deleteApp: async (app) => undefined,
});

export const useStorage = () => useContext(StorageContext);
