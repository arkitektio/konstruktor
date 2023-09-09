import { ChildProcess } from "@tauri-apps/api/shell";
import { CommandParams, useCommand } from "./hooks/useCommand";
import { callbackify } from "util";
import { useState } from "react";
import { Button } from "./components/ui/button";

export const CommandButton = (props: {
  params: CommandParams;
  title: string;
  callback?: (x: ChildProcess) => void;
  runningTitle?: string;
}) => {
  const { run, logs, error, finished, running } = useCommand(props.params);

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
              alert(x.stderr);
            }
          });
        }}
        disabled={running}
        className={`border cursor-pointer shadow-md shadow hover:bg-gray-200 border-1 border-gray-300 rounded p-2 bg-white text-black ${
          running ? "animate-pulse" : ""
        }`}
      >
        {running && props.runningTitle ? props.runningTitle : props.title}
        {finished?.code == 1 && <div className="text-red-400">Error</div>}
      </Button>
    </>
  );
};

export const DangerousCommandButton = (props: {
  params: CommandParams;
  title: string;
  callback?: (x: ChildProcess) => void;
  runningTitle?: string;
  to?: number;
}) => {
  const { run, logs, error, finished, running } = useCommand(props.params);
  const [countDown, setCountDown] = useState<number>(1);
  const to = props.to || 10;
  return (
    <>
      <Button
        onClick={() => {
          console.log("run");
          if (countDown == to) {
            setCountDown(1);
            run().then((x) => {
              console.log(x);
              if (x.code == 0) {
                props.callback && props.callback(x);
              } else {
                alert(x.stderr);
              }
            });
          } else {
            setCountDown(countDown + 1);
          }
        }}
        disabled={running}
        className={`border cursor-pointer shadow-md shadow bg-red-200 hover:bg-red-400 ${
          countDown > 1 && "bg-red-400"
        } border-1 border-gray-300 rounded p-2 bg-white text-black ${
          running ? "animate-pulse" : ""
        }`}
      >
        {running && props.runningTitle ? props.runningTitle : props.title}
        {countDown > 1 &&
          countDown < 11 &&
          ` ( ${to - countDown} more clicks to confirm)`}
      </Button>
    </>
  );
};
