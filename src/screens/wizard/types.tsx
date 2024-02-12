import type { FormikErrors, FormikHandlers, FormikValues } from "formik";

export enum DockerConnectionStrategy {
  LOCAL = "LOCAL",
  REMOTE = "REMOTE",
}

export type StepProps<T extends any = {}> = {
  errors: FormikErrors<FormikValues>;
  values: FormikValues;
  schema: T
  handleChange: FormikHandlers["handleChange"];
};

export type DockerConfig = {
  strategy: DockerConnectionStrategy;
  addr?: string;
};

export type DockerApiStatus = {
  version: string;
  memory: number;
};

export type InstalledApp = {
  name: string;
  path: string;
};
