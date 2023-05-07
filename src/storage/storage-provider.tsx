import { forage } from "@tauri-apps/tauri-forage";
import React, { useEffect, useState } from "react";
import { SetupValues } from "../screens/wizard/Setup";
import { App, StorageContext } from "../storage/storage-context";
import { stringify, parse } from "yaml";
import {
  createDir,
  BaseDirectory,
  writeTextFile,
  readDir,
  readTextFile,
  removeDir,
} from "@tauri-apps/api/fs";
import { InstalledApp } from "../screens/wizard/types";

const StorageProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [availableApps, setAvailableApps] = useState<InstalledApp[]>([]);
  const [reset, setReset] = useState(false);

  const installApp = async (app: SetupValues) => {
    await createDir(`apps/${app.name}`, {
      dir: BaseDirectory.App,
      recursive: true,
    });
    await writeTextFile(`apps/${app.name}/setup.yaml`, stringify(app), {
      dir: BaseDirectory.App,
    });
    setReset(!reset);
  };

  const deleteApp = async (app: App) => {
    await removeDir(`apps/${app.name}`, {
      dir: BaseDirectory.App,
      recursive: true,
    });
    setReset(!reset);
  };

  const listAppDir = async () => {
    let entries = await readDir("apps", { dir: BaseDirectory.App });
    console.log(entries);
    let apps: InstalledApp[] = [];

    for (let entry of entries) {
      if (entry.children) {
        let setup_string = await readTextFile(`apps/${entry.name}/setup.yaml`, {
          dir: BaseDirectory.App,
        });
        console.log(setup_string);
        let setup = parse(setup_string) as SetupValues;
        console.log(setup);
        apps.push({ name: setup.name, path: entry.path });
      }
    }
    setAvailableApps(apps);
  };

  useEffect(() => {
    listAppDir();
  }, [reset]);

  return (
    <StorageContext.Provider
      value={{
        apps: availableApps,
        installApp: installApp,
        deleteApp,
      }}
    >
      {children}
    </StorageContext.Provider>
  );
};

export { StorageProvider };
