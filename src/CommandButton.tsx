import { ChildProcess } from "@tauri-apps/api/shell";
import { CommandParams, useCommand } from "./hooks/useCommand";
import { callbackify } from "util";
import { useState } from "react";
import { Button } from "./components/ui/button";
import { useAlerter } from "./alerter/alerter-context";
import { Popover, PopoverContent } from "./components/ui/popover";
import { PopoverClose, PopoverTrigger } from "@radix-ui/react-popover";

export const CommandButton = (props: {
  params: CommandParams;
  title: string;
  callback?: (x: ChildProcess) => void;
  runningTitle?: string;
}) => {
  const { run, logs, error, finished, running } = useCommand(props.params);
  const { alert } = useAlerter();

  return (
    <>
      <Button
        onClick={() => {
          console.log("run");
          run().then((x) => {
            console.log(x);
            if (x.code == 0) {
              props.callback && props.callback(x);
            } else {
              alert({
                error: `Error while running ${props.title}`,
                message: x.stderr,
                subtitle: x.stdout,
              });
            }
          });
        }}
        disabled={running}
        className={`border cursor-pointer shadow-md shadow hover:bg-gray-200 border-1 border-gray-300 rounded p-2 bg-white text-black ${
          running ? "animate-pulse" : ""
        }`}
      >
        {running && props.runningTitle ? props.runningTitle : props.title}
      </Button>
    </>
  );
};

export const DangerousCommandButton = (props: {
  params: CommandParams;
  title: string;
  callback?: (x: ChildProcess) => void;
  confirmTitle?: string;
  confirmDescription?: string;
  runningTitle?: string;
  to?: number;
}) => {
  const { run, logs, error, finished, running } = useCommand(props.params);
  const { alert } = useAlerter();
  const [open, setOpen] = useState(false);
  const to = props.to || 10;
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button onClick={() => setOpen(true)}>
          {running && props.runningTitle ? props.runningTitle : props.title}
        </Button>
      </PopoverTrigger>
      <PopoverContent>
        <div className="flex flex-col gap-1">
          <div className="text-md">{props.confirmTitle || "Are you sure?"}</div>
          <div className="text-xs text-muted-foreground">
            {props.confirmDescription || "This might cause unexpected results"}
          </div>
          <div className="flex flex-row gap-2 w-full mt-2">
            <Button
              className="w-full"
              onClick={() => {
                console.log("run");
                run().then((x) => {
                  console.log(x);
                  if (x.code == 0) {
                    setOpen(false);
                    props.callback && props.callback(x);
                  } else {
                    setOpen(false);
                    alert({
                      error: `Error while running ${props.title}`,
                      message: x.stderr,
                      subtitle: x.stdout,
                    });
                  }
                });
              }}
            >
              Yes
            </Button>

            <PopoverClose asChild>
              <Button className="w-full">No</Button>
            </PopoverClose>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
};
