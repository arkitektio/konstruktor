import React, { useEffect, useState } from "react";
import { useCommunication } from "../../../communication/communication-context";
import {
  DockerApiStatus,
  DockerConfig,
  DockerConnectionStrategy,
  DockerInterfaceStatus,
  StepProps,
} from "../types";

export const CheckDocker: React.FC<StepProps> = (props) => {
  const { call } = useCommunication();
  const [showVersion, setShowVersion] = useState(false);
  const [dockerApiStatus, setDockerApiStatus] =
    useState<DockerApiStatus | null>(null);
  const [dockerInterfaceStatus, setDockerInterfaceStatus] =
    useState<DockerInterfaceStatus | null>(null);
  const [advertise, setAdvertise] = useState<boolean>(false);

  useEffect(() => {
    call<DockerConfig, DockerApiStatus>("test_docker", {
      strategy: DockerConnectionStrategy.LOCAL,
    }).then((res) => setDockerApiStatus(res));
  }, []);

  useEffect(() => {
    call<DockerConfig, DockerInterfaceStatus>("docker_version_cmd", {
      strategy: DockerConnectionStrategy.LOCAL,
    }).then((res) => setDockerInterfaceStatus(res));
  }, []);

  return (
    <div className="text-center h-full my-7">
      <div className="font-light text-3xl mt-4">
        Lets see if we can find docker..
      </div>
      <div className="">
        {dockerInterfaceStatus ? (
          <div className="text-center mt-6">
            <div className="text-7xl mb-6">🎉</div>
            Beautfiul, we found docker on your system! <br></br>
            <button
              onClick={() => setShowVersion(!showVersion)}
              className="border rounded border-gray-700 p-1"
            >
              {showVersion ? "Hide" : "Show"} Version{" "}
            </button>
            <p className="font-light text-sm mt-6">
              {showVersion && dockerInterfaceStatus.ok}
            </p>
            <div className="mt-6">
              {dockerApiStatus ? (
                <>
                  It appears that docker is installed locally. That's great. You
                  will be able to monitor the services in Konstruktor
                </>
              ) : (
                <>
                  It appears that the docker cli is installed. You will be to
                  install arkitekt on this system, but you will not be able to
                  monitor your installation in Konstruktor
                </>
              )}
            </div>
          </div>
        ) : (
          <div className="text-center mt-6">
            <div className="text-7xl mb-6">😩</div>
            Docker seems to be not installed on your system. This won't work!
            Please make sure docker is installed and running on your system.
            Follow the instructions here to get you started.
          </div>
        )}
      </div>
    </div>
  );
};
