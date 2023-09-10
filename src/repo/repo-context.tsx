import React, { useContext } from "react";

export type App = {
  name: string;
  long?: string;
  logo?: string;
  description?: string;
  identifier: string;
  version?: string;
  scopes?: string[];
  requires?: string[];
  client_kind: "WEBSITE" | "DEVELOPMENT";
  experimental?: boolean;
  redirect_uris?: string[];
};

export type Service = {
  name: string;
  interface: string;
  logo?: string;
  long?: string;
  description?: string;
  image: string;
  requires?: string[];
  experimental?: boolean;
};

export type AvailableForms =
  | "greeting"
  | "check_docker"
  | "service_selection"
  | "app_storage"
  | "app_selection"
  | "attention_superuser"
  | "done"
  | "admin_user"
  | "groups"
  | "users"
  | "scale"
  | "channel";

export type Group = {
  name: string;
  description: string;
};

export type User = {
  username: string;
  name?: string;
  password: string;
  email?: string;
  groups: string[];
};

export type SetupValues = {
  name: string;
  admin_username: string;
  admin_password: string;
  admin_email: string;
  attention: boolean;
  apps: App[];
  services: Service[];
  groups: Group[];
  users: User[];
};

export type Feature = {
  name: string;
  description: string;
  long: string;
};

export type Channel = {
  title: string;
  name: string;
  builder: string;
  logo: string;
  long: string;
  experimental: boolean;
  description: string;
  features: Feature[];
  forms: AvailableForms[];
  defaults: Partial<SetupValues>;
  availableApps: App[];
  availableServices: Service[];
};

export type RepoContextType = {
  channels: Channel[];
  ensureChannels: (channels: Channel[]) => void;
};

export const RepoContext = React.createContext<RepoContextType>({
  channels: [],
  ensureChannels: () => {},
});

export const useRepo = () => useContext(RepoContext);
