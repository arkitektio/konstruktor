import React, { useEffect, useState } from "react";
import {
  CommunicationContext,
  DockerConnectionStrategy,
  DockerInterfaceStatus,
  DockerStatus,
} from "./communication-context";
import { invoke } from "@tauri-apps/api/tauri";
export type ICommunicationProviderProps = {
  children: React.ReactNode;
};

const CommunicationProvider: React.FC<ICommunicationProviderProps> = ({
  children,
}) => {
  function call(channel: string, options?: any) {
    return invoke(channel, {
      event: JSON.stringify(options || {}),
    }).then((res: unknown) => JSON.parse(res as string));
  }

  const [dockerStatus, setDockerStatus] = useState<DockerStatus | null>(null);
  const [dockerInterfaceStatus, setDockerInterfaceStatus] =
    useState<DockerInterfaceStatus | null>(null);

  useEffect(() => {
    invoke("test_docker", {
      strategy: DockerConnectionStrategy.LOCAL,
    }).then((res) => setDockerStatus(res as DockerStatus));
  }, []);

  useEffect(() => {
    call("docker_version_cmd", {
      strategy: DockerConnectionStrategy.LOCAL,
    }).then((res) => setDockerInterfaceStatus(res));
  }, []);

  return (
    <CommunicationContext.Provider
      value={{
        call: call,
        status: dockerStatus,
        interfaceStatus: dockerInterfaceStatus,
      }}
    >
      {children}
    </CommunicationContext.Provider>
  );
};

export { CommunicationProvider };
