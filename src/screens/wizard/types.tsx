import type { FormikErrors, FormikHandlers, FormikValues } from "formik";

export enum DockerConnectionStrategy {
  LOCAL = "LOCAL",
  REMOTE = "REMOTE",
}

export type StepProps = {
  errors: FormikErrors<FormikValues>;
  values: FormikValues;
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

export type DockerInterfaceStatus = {
  ok: string;
  error: string;
};

export type InstalledApp = {
  name: string;
  path: string;
};
