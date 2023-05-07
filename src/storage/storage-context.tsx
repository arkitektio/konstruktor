import React, { useContext } from "react";
import { SetupValues } from "../screens/wizard/Setup";

export type App = {
  path: string;
  name: string;
};

export type StorageContextType = {
  apps: App[];
  installApp: (app: SetupValues) => Promise<void>;
  deleteApp: (app: App) => Promise<void>;
};

export const StorageContext = React.createContext<StorageContextType>({
  apps: [],
  installApp: async (app) => undefined,
  deleteApp: async (app) => undefined,
});

export const useStorage = () => useContext(StorageContext);
