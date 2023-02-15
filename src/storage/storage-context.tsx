import React, { useContext } from "react";
import { SetupValues } from "../screens/wizard/Setup";

export type App = {
  dirpath: string;
  name: string;
};

export type StorageContextType = {
  apps: SetupValues[];
  installApp: (app: SetupValues) => void;
  deleteApp: (app: SetupValues) => void;
  deleteAllApps: () => void;
};

export const StorageContext = React.createContext<StorageContextType>({
  apps: [],
  installApp: (app) => null,
  deleteApp: (app) => null,
  deleteAllApps: () => undefined,
});

export const useStorage = () => useContext(StorageContext);
