import { forage } from "@tauri-apps/tauri-forage";
import React, { useEffect, useState } from "react";
import { SetupValues } from "../screens/wizard/Setup";
import { App, StorageContext } from "./storage-context";

const StorageProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [availableApps, setAvailableApps] = useState<SetupValues[]>([]);

  const installApp = async (app: SetupValues) => {
    let newapps = [...availableApps, app];

    await forage.setItem({
      key: "installed_apps",
      value: JSON.stringify(newapps),
    })();
    setAvailableApps(newapps);
  };

  const deleteApp = async (app: SetupValues) => {
    let newapps = availableApps.filter((c) => c.app_path == app.app_path);

    await forage.setItem({
      key: "installed_apps",
      value: JSON.stringify(newapps),
    })();
    setAvailableApps(newapps);
  };

  const deleteAllApps = async () => {
    let newapps = [] as SetupValues[];

    await forage.setItem({
      key: "installed_apps",
      value: JSON.stringify(newapps),
    })();
    setAvailableApps(newapps);
  };

  useEffect(() => {
    forage
      .getItem({ key: "installed_apps" })()
      .then((value) => {
        console.log(value);
        setAvailableApps(JSON.parse(value) || []);
      });
  }, []);

  return (
    <StorageContext.Provider
      value={{
        apps: availableApps,
        installApp: installApp,
        deleteApp,
        deleteAllApps,
      }}
    >
      {children}
    </StorageContext.Provider>
  );
};

export { StorageProvider };
