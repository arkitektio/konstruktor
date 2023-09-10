import { forage } from "@tauri-apps/tauri-forage";
import React, { useEffect, useState } from "react";
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
    await createDir(`apps/${name}`, {
      dir: BaseDirectory.App,
      recursive: true,
    });
    await writeTextFile(`apps/${name}/setup.yaml`, stringify(values), {
      dir: BaseDirectory.App,
    });
    await writeTextFile(`apps/${name}/channel.yaml`, stringify(channel), {
      dir: BaseDirectory.App,
    });

    let entries = await readDir("apps", { dir: BaseDirectory.App });

    let path = entries.find((x) => x.name == name)?.path;
    if (!path) {
      throw new Error("Could not find path");
    }
    setReset(!reset);
    return path;
  };

  const deleteApp = async (name: string) => {
    await removeDir(`apps/${name}`, {
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
        let setup = parse(setup_string) as SetupValues;
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
