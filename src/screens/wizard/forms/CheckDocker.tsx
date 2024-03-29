import React, { useState } from "react";
import { useCommunication } from "../../../communication/communication-context";
import { Alert } from "../../../components/ui/alert";
import { Button } from "../../../components/ui/button";
import { StepProps } from "../types";
import {open} from "@tauri-apps/api/shell"

export const CheckDocker: React.FC<StepProps> = (props) => {
  const { call, status, interfaceStatus } = useCommunication();
  const [showVersion, setShowVersion] = useState(false);

  return (
    <div className="h-full w-full my-7 flex flex-col">
      <div className="font-light text-7xl"> About Docker!</div>
      <div className="font-light text-2xl mt-4">
        Let's check if docker is installed
      </div>
      <div className="mb-2 text-justify mt-4 max-w-xl">
        This manager works with docker. Docker is a container runtime that allows us
        to run software completely isolated from your system. Like this we can
        ensure that Arkitekt does not interfere with your system and that you
        can run multiple deployments on the same system.
      </div>
      <div className="max-w-xl">
        {interfaceStatus ? (
          <div className="mt-6">
            <div className="text-7xl mb-6">🎉</div>
            Beautiful, we found docker on your system! <br></br>
            <Button
              variant="outline"
              className="mt-6"
              onClick={() => setShowVersion(!showVersion)}
            >
              {showVersion ? "Hide" : "Show"} Version{" "}
            </Button>
            <p className="font-light text-sm mt-6">
              {showVersion && interfaceStatus.ok}
            </p>
            <div className="mt-6">
              {status?.connected ? (
                <Alert variant="default">
                  Double lucky! It appears that the docker api is also
                  accessible. You will be able to monitor your installation in Konstruktor. 🎉
                </Alert>
              ) : (
                <Alert variant="destructive">
                  Not that lucky! It appears that the docker api is not
                  accessible (probably because your docker deamon is either not
                  running or you don't run a local instance). You will be to
                  install arkitekt on this system, but you will not be able to
                  monitor your installation in Konstruktor
                </Alert>
              )}
            </div>
          </div>
        ) : (
          <div className="mt-6">
            <div className="text-7xl mb-6">😩</div>
            Docker seems to be not installed on your system. This won't work!
            Please make sure docker is installed and running on your system.
            Follow the instructions in the documentation to get you started.
            <div className="mt-6">
              <Button>
                <div onClick={() => open("https://arkitekt.live/docs/installation")} >
                  Open Documentation
                </div>
              </Button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};
