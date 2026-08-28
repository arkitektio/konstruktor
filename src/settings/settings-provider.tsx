import { forage } from "@tauri-apps/tauri-forage";
import React, { useEffect, useState } from "react";
import {
  applyBrand,
  clearBrand,
  DEFAULT_BRAND_CHROMA,
  DEFAULT_BRAND_HUE,
} from "../lib/brand";
import { Settings, SettingsContext, defaultSettings } from "./settings-context";

/** The key the pre-paint script in index.html reads, so the two cannot drift. */
const THEME_KEY = "konstruktor-theme";

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
          // Merge over the defaults: settings written by an older version are missing
          // keys this one reads, and an undefined value would surface in the UI.
          setActiveSettings({ ...defaultSettings, ...JSON.parse(value) });
        }
      });
  }, [reset]);

  /**
   * The `light`/`dark` class on <html>, and the localStorage mirror the pre-paint
   * script reads on the next launch.
   *
   * `system` is resolved here rather than left to CSS because the theme also decides
   * `color-scheme`, which is what stops the webview painting a white frame behind a
   * dark window — and because the pre-paint script has to resolve it too.
   */
  useEffect(() => {
    const root = window.document.documentElement;
    const theme =
      settings.theme === "system"
        ? window.matchMedia("(prefers-color-scheme: dark)").matches
          ? "dark"
          : "light"
        : settings.theme;

    root.classList.remove("light", "dark");
    root.classList.add(theme);
    root.style.colorScheme = theme;

    try {
      // The preference, not the resolution: "system" has to stay "system" across a
      // restart, or a machine that changed its appearance would keep the old one.
      localStorage.setItem(THEME_KEY, settings.theme);
    } catch {
      /* localStorage unavailable */
    }
  }, [settings.theme]);

  /**
   * Follow the OS while the preference is "system" — a desktop window is open long
   * enough to be sitting there when the machine switches at sunset.
   */
  useEffect(() => {
    if (settings.theme !== "system") return;

    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const root = window.document.documentElement;
      const theme = media.matches ? "dark" : "light";
      root.classList.remove("light", "dark");
      root.classList.add(theme);
      root.style.colorScheme = theme;
    };

    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [settings.theme]);

  /**
   * The brand tokens.
   *
   * "No preference" clears the inline properties rather than writing today's defaults
   * into them: leaving `--brand-hue` and `--brand-chroma-user` unset is what lets the
   * stylesheet's own values apply, so a later change to the stock palette still reaches
   * anyone who never picked a colour.
   */
  useEffect(() => {
    if (settings.brandHue === null && settings.brandChroma === null) {
      clearBrand();
      return;
    }
    applyBrand({
      hue: settings.brandHue ?? DEFAULT_BRAND_HUE,
      chroma: settings.brandChroma ?? DEFAULT_BRAND_CHROMA,
    });
  }, [settings.brandHue, settings.brandChroma]);

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
