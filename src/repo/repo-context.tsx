import React, { useContext } from "react";
import * as  Yup from "yup";
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

export type ClientKind = "WEBSITE" | "DEVELOPMENT";

export const clientKindOptions: ClientKind[] = ["WEBSITE", "DEVELOPMENT"];

export type Service = {
  name: string;
  image: string;
  interface: string;
  logo?: string;
  long?: string;
  description?: string;
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
  | "channel"
  | "check_gpu";


export const availableFormsValue: AvailableForms[] = [
  "greeting",
  "check_docker",
  "service_selection",
  "app_storage",
  "app_selection",
  "attention_superuser",
  "done",
  "admin_user",
  "groups",
  "users",
  "scale",
  "channel",
  "check_gpu",
];

export type Group = {
  name: string;
  description?: string;
};

export type User = {
  username: string;
  password: string;
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



export const appSchema = Yup.object().shape({
  name: Yup.string().required("Name is required"),
  long: Yup.string(),
  logo: Yup.string(),
  description: Yup.string().required("Description is required"),
  identifier: Yup.string().required("Identifier is required"),
  version: Yup.string().required("Version is required"),
  scopes: Yup.array().of(Yup.string().required()).required("Scopes are required"),
  requires:  Yup.array().of(Yup.string().required()),
  client_kind: Yup.string().oneOf(clientKindOptions).required(),
  experimental: Yup.boolean(),
  redirect_uris: Yup.array().of(Yup.string().required()),
});

export const serviceSchema = Yup.object().shape({
  name: Yup.string().required("Name is required"),
  image: Yup.string().required("Image is required"),
  long: Yup.string(),
  logo: Yup.string(),
  description: Yup.string(),
  requires:  Yup.array().of(Yup.string().required()),
  experimental: Yup.boolean(),
  interface: Yup.string().required("Interface is required"),
});


export const featureSchema = Yup.object().shape({
  name: Yup.string(),
  description: Yup.string(),
  long: Yup.string(),
});

export const availableForms = Yup.array().of<AvailableForms>(Yup.string().oneOf(availableFormsValue).required()).required("Forms are required");


export type Channel = {
  title: string;
  name: string;
  builder: string;
  logo?: string;
  long?: string;
  experimental?: boolean;
  description?: string;
  features?: Feature[];
  forms: AvailableForms[];
  defaults?: Partial<SetupValues>;
  availableApps?: App[];
  availableServices?: Service[];
};


const usersSchema = Yup.array().of(
  Yup.object().shape({
    username: Yup.string().required("Username is required"),
    password: Yup.string().required("Password is required"),
    groups: Yup.array(Yup.string().required()).required("Groups are required"),
  })
)

const groupsSchema = Yup.array().of(
  Yup.object().shape({
    name: Yup.string().required("Name is required"),
    description: Yup.string(),
  })
)

export const setupSchema = Yup.object().shape({
  name: Yup.string(),
  adminUsername: Yup.string(),
  adminPassword: Yup.string(),
  adminEmail: Yup.string(),
  attention: Yup.boolean(),
  users: usersSchema,
  groups: groupsSchema
})



export const channelSchema = Yup.object().shape({
  name: Yup.string()
    .required("Name is required")
    .min(3, "Name must be at least 3 characters")
    .max(20, "Name must be at most 20 characters"),
  builder: Yup.string()
    .required("Builder is required"),
  availableApps: Yup.array().of(appSchema),
  availableServices: Yup.array().of(serviceSchema),
  logo: Yup.string(),
  long: Yup.string(),
  experimental: Yup.boolean(),
  defaults: setupSchema,
  description: Yup.string(),
  title: Yup.string().required("Title is required"),
  forms: availableForms,

});
  



export const repoSchema = Yup.object().shape({
  repo: Yup.string()
    .required("Name is required")
    .min(3, "Name must be at least 3 characters"),
  channels: Yup.array().of(channelSchema).required("Channels are required"),
});



export type RepoError = {
  repo: string;
  errors: Yup.ValidationError[];
};

export type RepoContextType = {
  channels: Channel[];
  errors: RepoError[];
  ensureChannels: (channels: Channel[]) => void;
  validate: (jsonData: any) => void;
};

export const RepoContext = React.createContext<RepoContextType>({
  channels: [],
  errors: [],
  ensureChannels: () => {},
  validate: (jsonData: any) => {},
});

export const useRepo = () => useContext(RepoContext);
