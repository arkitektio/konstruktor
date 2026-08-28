import React, { useContext } from "react";

/** The coordination server offered first when a new hub is created. */
export const DEFAULT_COORDINATION_SERVER = "go.arkitekt.live";

/** Matches the classes `settings-provider` puts on <html>, plus "follow the OS". */
export type Theme = "light" | "dark" | "system";

export type Settings = {
  theme: Theme;
  /** Pre-filled on the hub wizard; every hub still stores its own. */
  coordinationServer: string;
  /**
   * Servers the user has picked before, offered alongside the default one. Kept here
   * rather than per-deployment because the point is to not retype an address that a
   * second hub will use as well.
   */
  knownCoordinationServers: string[];
  /**
   * The brand hue and chroma, in the same scale Kontrol stores them in — so a user who
   * tinted their organization there and this installer here sees one colour, not two.
   * `null` means "the Arkitekt default", which is what leaves `--brand-chroma-user`
   * unset and lets each theme derive its own chroma (see `src/lib/brand.ts`).
   */
  brandHue: number | null;
  brandChroma: number | null;
  /**
   * Where to ask what address the internet sees this machine as.
   *
   * Empty by default, and deliberately so: every other request konstruktor makes goes to
   * the coordination server the user named, and this one would tell a stranger their IP.
   * It is a convenience for the address step, not something to switch on for them.
   */
  egressEndpoint: string;
  /**
   * A service that will fetch a URL on request and report what it got, used to check
   * whether this hub answers from outside.
   *
   * Also empty by default. Nothing is checked without one, which the picker says plainly
   * rather than showing a cross nobody can act on.
   */
  proberEndpoint: string;
};

export type SettingContext = {
  settings: Settings;
  setSettings: (settings: Settings) => Promise<void>;
};

export const defaultSettings: Settings = {
  theme: "system",
  coordinationServer: DEFAULT_COORDINATION_SERVER,
  knownCoordinationServers: [],
  brandHue: null,
  brandChroma: null,
  egressEndpoint: "",
  proberEndpoint: "",
};

export const SettingsContext = React.createContext<SettingContext>({
  settings: defaultSettings,
  setSettings: async () => undefined,
});

export const useSettings = () => useContext(SettingsContext);
