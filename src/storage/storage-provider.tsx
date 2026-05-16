import { forage } from "@tauri-apps/tauri-forage";
import React, { useEffect, useState } from "react";
import { App, StorageContext } from "../storage/storage-context";
import { stringify, parse } from "yaml";
import {
  mkdir,
  BaseDirectory,
  writeTextFile,
  readDir,
  readTextFile,
  remove,
} from "@tauri-apps/plugin-fs";
import { appDataDir, join } from "@tauri-apps/api/path";
import { InstalledApp } from "../screens/wizard/types";
import { Channel, SetupValues } from "../repo/repo-context";

const StorageProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [availableApps, setAvailableApps] = useState<InstalledApp[]>([]);
  const [reset, setReset] = useState(false);

  const installApp = async (
    name: string,
    channel: Channel,
    values: Partial<SetupValues>
  ) => {
    await mkdir(`apps/${name}`, {
      baseDir: BaseDirectory.AppData,
      recursive: true,
    });
    await writeTextFile(`apps/${name}/setup.yaml`, stringify(values), {
      baseDir: BaseDirectory.AppData,
    });
    await writeTextFile(`apps/${name}/channel.yaml`, stringify(channel), {
      baseDir: BaseDirectory.AppData,
    });

    const basePath = await appDataDir();
    const appPath = await join(basePath, "apps", name);
    setReset(!reset);
    return appPath;
  };

  const deleteApp = async (name: string) => {
    await remove(`apps/${name}`, {
      baseDir: BaseDirectory.AppData,
      recursive: true,
    });
    setReset(!reset);
  };

  const listAppDir = async () => {
    let entries = await readDir("apps", { baseDir: BaseDirectory.AppData });
    console.log(entries);
    const basePath = await appDataDir();
    let apps: InstalledApp[] = [];

    for (let entry of entries) {
      if (entry.isDirectory) {
        let setup_string = await readTextFile(`apps/${entry.name}/setup.yaml`, {
          baseDir: BaseDirectory.AppData,
        });
        try {
          let setup = parse(setup_string) as SetupValues;
          if (entry.name) {
            const appPath = await join(basePath, "apps", entry.name);
            apps.push({ name: entry.name, path: appPath });
          }
        } catch (e) {
          console.error(e);
        }
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
