import React, { useContext } from "react";
import * as  Yup from "yup";
import * as zod from "zod";
import { pluginSchema } from "../screens/wizard/plugins/global";
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



export const featureSchema = Yup.object().shape({
  name: Yup.string(),
  description: Yup.string(),
  long: Yup.string(),
});

export const availableForms = Yup.array().of<AvailableForms>(Yup.string().oneOf(availableFormsValue).required()).required("Forms are required");




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










export const channelSchema = zod.object({
  name: zod.string(),
  builder: zod.string(),
  logo: zod.optional(zod.string()),
  long: zod.optional(zod.string()),
  experimental: zod.optional(zod.boolean()),
  description: zod.optional(zod.string()),
  title: zod.optional(zod.string()),
  plugins: zod.array(pluginSchema),
  forms: zod.array(zod.string()),
  defaults: zod.any()
})


export type Channel = zod.TypeOf<typeof channelSchema>



export const repoSchema = zod.object({
  repo: zod.string(),
  channels: zod.array(channelSchema)

})


export type Repo = zod.TypeOf<typeof repoSchema>










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
