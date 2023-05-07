import React, { useContext } from "react";
import { SetupValues } from "../screens/wizard/Setup";

export type Settings = {
  baker: string;
};

export type SettingContext = {
  settings: Settings;
  setSettings: (settings: Settings) => Promise<void>;
};

export const defaultSettings = {
  baker: "jhnnsrs/guss:prodx",
};
export const SettingsContext = React.createContext<SettingContext>({
  settings: defaultSettings,
  setSettings: async (settings) => undefined,
});

export const useSettings = () => useContext(SettingsContext);
