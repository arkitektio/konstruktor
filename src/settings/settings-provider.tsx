import { forage } from "@tauri-apps/tauri-forage";
import React, { useEffect, useState } from "react";
import { SetupValues } from "../screens/wizard/Setup";
import { Settings, SettingsContext, defaultSettings } from "./settings-context";
import { stringify, parse } from "yaml";
import {
  createDir,
  BaseDirectory,
  writeTextFile,
  readDir,
  readTextFile,
} from "@tauri-apps/api/fs";
import { InstalledApp } from "../screens/wizard/types";

export const SettingsProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [settings, setActiveSettings] = useState<Settings>(defaultSettings);
  const [reset, setReset] = useState(false);

  const setSettings = async (settings: Settings) => {
    await forage.setItem({
      key: "settings",
      value: JSON.stringify(settings),
    })();
    setReset(!reset);
  };

  useEffect(() => {
    forage
      .getItem({ key: "settings" })()
      .then((value) => {
        if (value) {
          setActiveSettings(JSON.parse(value));
        }
      });
  }, [reset]);

  return (
    <SettingsContext.Provider
      value={{
        settings,
        setSettings,
      }}
    >
      {children}
    </SettingsContext.Provider>
  );
};
