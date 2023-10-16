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
    invoke("test_docker", {event: JSON.stringify({
      strategy: DockerConnectionStrategy.LOCAL,
    })}).then((res) => {
      let result = JSON.parse(res as string);
    


      let new_status = {connected: true, memory: parseInt(result.memory), version: result.version, error: ""}
      console.error("The new status", new_status)
      setDockerStatus(new_status);

    })
    
    
    
    .catch((e) => {
      console.error("Docker interface error", e);
      setDockerStatus({
        connected: false,
        version: "unknown",
        error: e,
        memory: 0,
      });
    });
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
