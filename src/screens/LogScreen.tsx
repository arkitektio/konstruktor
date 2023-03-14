import React, { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { useStorage } from "../storage/storage-context";
import { SetupValues } from "./wizard/Setup";

import { Command } from "@tauri-apps/api/shell";

export const Logs: React.FC<{ app: SetupValues }> = ({ app }) => {
  const [rollingLog, setRollingLog] = useState<string[]>([]);
  const [retrigger, setRetrigger] = useState<boolean>(false);
  const lok_port = 8000;
  let deployment = app.name;

  const log = (line: string) => {
    setRollingLog((prev) => {
      return [line, ...prev].slice(0, 50);
    });
  };

  const runDocker = async (args: string[]) => {
    const command = new Command("docker", args, {
      cwd: app.app_path,
    });
    command.stdout.on("data", (line) => log(`"${line}"`));

    let child = await command.execute();
    return child;
  };

  useEffect(() => {
    runDocker(["compose", "logs"]).then((child) => {
      setTimeout(() => {
        setRetrigger(!retrigger);
      }, 4000);
    });
  }, [retrigger]);

  return (
    <div className="h-full w-full relative">
      <div className="text-xl flex flex-row bg-back-800 text-white shadow-xl mb-2 p-2 ">
        <div className="flex-1 my-auto ">
          <Link to={`/dashboard/${app.name}`}>{"< Back"}</Link>
        </div>
        <div className="flex-grow my-auto text-center">{app.name}</div>
        <div className="flex-1 my-auto text-right">
          <Link to={`/logs/${app.name}`}>Logs</Link>
        </div>
      </div>
      <div className="font-light mt-2">Log on</div>
      <pre>
        {rollingLog.map((l, index) => (
          <div key={index}>
            {l}
            <br />
          </div>
        ))}
      </pre>
    </div>
  );
};

export const LogScreen: React.FC<{}> = (props) => {
  const { id } = useParams<{ id: string }>();
  const { apps } = useStorage();

  let app = apps.find((app) => app.name === id);

  return app ? <Logs app={app} /> : <>Could not find this app</>;
};
