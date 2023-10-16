import React, { useContext } from "react";

export enum DockerConnectionStrategy {
  LOCAL = "LOCAL",
  REMOTE = "REMOTE",
}

export type DockerConfig = {
  strategy: DockerConnectionStrategy;
  addr?: string;
};

export type DockerInterfaceStatus = {
  ok: string;
  error: string;
};

export type DockerStatus = {
  connected: boolean;
  error: string;
  version: string;
  memory: number;
};

export interface CommunicationContextType {
  call<Options extends {}, Result extends {}>(
    channel: string,
    options?: Options
  ): Promise<Result>;
  status: DockerStatus | null;
  interfaceStatus: DockerInterfaceStatus | null;
}

export const CommunicationContext =
  React.createContext<CommunicationContextType>({
    call: null as unknown as any,
    status: null,
    interfaceStatus: null,
  });

export const useCommunication = () => useContext(CommunicationContext);
