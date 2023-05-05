import React, { useContext } from "react";
import { SetupValues } from "../screens/wizard/Setup";

export type App = {
  dirpath: string;
  name: string;
};

export type StorageContextType = {
  apps: SetupValues[];
  installApp: (app: SetupValues) => Promise<SetupValues[]>;
  deleteApp: (app: SetupValues) => Promise<SetupValues[]>;
  deleteAllApps: () => Promise<SetupValues[]>;
};

export const StorageContext = React.createContext<StorageContextType>({
  apps: [],
  installApp: async (app) => [app],
  deleteApp: async (app) => [],
  deleteAllApps: async () => [],
});

export const useStorage = () => useContext(StorageContext);
