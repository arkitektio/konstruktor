import React, { useState } from "react";
import { useCommunication } from "../../../communication/communication-context";
import { Alert } from "../../../components/ui/alert";
import { Button } from "../../../components/ui/button";
import { StepProps } from "../types";

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
        Arkitekt works with docker. Docker is a container runtime that allows us
        to run arkitekt completely isolated from your system. Like this we can
        ensure that arkitekt does not interfere with your system and that you
        can run multiple arkitekt instances on the same system.
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
                <>
                  Double lucky! It appears that the docker api is also
                  accessible. You will be to install arkitekt on this system,
                  and monitor your installation in Konstruktor.
                </>
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
                <a href="https://jhnnsrs.github.com/doks/installation">
                  Open Documentation
                </a>
              </Button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};
