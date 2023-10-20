import React, { useState } from "react";
import { useCommunication } from "../../../communication/communication-context";
import { Alert } from "../../../components/ui/alert";
import { Button } from "../../../components/ui/button";
import { StepProps } from "../types";
import { ScrollArea } from "@radix-ui/react-scroll-area";
import { useLazyCommand } from "../../../hooks/useCommand";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "../../../components/ui/collapsible";

export const CheckGPUDocker: React.FC<StepProps> = (props) => {
  const [checking, setChecking] = useState(false);
  const [checked, setChecked] = useState(false);

  const { run, logs, finished } = useLazyCommand({
    logLength: 50,
  });

  const check = async () => {

    setChecking(true);

    let pullResult = await run({
      program: "docker",
      args: ["pull", "nvidia/cuda:11.8.0-base-centos7"],
    });
    if (pullResult.code != 0) {
      throw Error("Error while pulling builder image");
    }
    await run({
      program: "docker",
      args: ["run", "--rm", "--gpus", "all", "nvidia/cuda:11.8.0-base-centos7", "nvidia-smi"],
    });

    setChecked(true);

    setChecking(false);
  };

  return (
    <div className="h-full w-full my-7 flex flex-col">
      <div className="font-light text-7xl">GPU 👩🏽‍💻</div>
      <div className="font-light text-2xl mt-4">
        
      </div>
      <div className="mb-2 text-justify mt-4 max-w-xl">
        Some functionality in this channels require GPU support that needs CUDA 11 or above. Let's
        check if your system supports this.<br/> <br/>


        In order to check if your system supports CUDA 11, we run a little docker
        container and check if your CUDA Drivers are correctly installed. This 
        check might take a few seconds.
      </div>
      <Button className="w-20" onClick={check}>{checking ? "Checking...": "Check"} </Button>
    {checked && !checking && <div className="mt-2 max-w-xl">
      {logs.join("\n").includes("failed") ? (
        <Alert variant="destructive">
          It appears that your system does not support CUDA 11. Please refer to the documentation
          on how to install the latest nvidia drivers on your system. Otherwise you can still use
          this channel, but you will not be able to install apps that require GPU support.
          </Alert>
      ) :
        <Alert variant="default" className="border-green-200">
          🎉 Looks like CUDA support is available. You can install apps that require GPU support.
        </Alert>
} 
    
    </div>}
      

    <Collapsible className="max-w-xl h-[50vh]">
      <CollapsibleTrigger className="mt-2">Show Logs</CollapsibleTrigger>
      <CollapsibleContent>
      {logs.length > 0 && (
              <ScrollArea className="max-w-xl h-[50vh] bg-gray-900 p-3 rounded rounded-md overflow-scroll">
                {logs.map((p, i) => (
                  <div className="w-full grid grid-cols-12 ">
                    <div className="col-span-1">{i}</div>
                    <div className="col-span-11"> {p}</div>
                  </div>
                ))}
              </ScrollArea>
            )}
      </CollapsibleContent>
    </Collapsible>  



     
    </div>
  );
};
