import React, { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { useStorage, App } from "../storage/storage-context";
import { SetupValues } from "./wizard/Setup";

import { Command } from "@tauri-apps/api/shell";
import { useCommand, useLazyCommand } from "../hooks/useCommand";

export const Logs: React.FC<{ app: App; service?: string }> = ({
  app,
  service,
}) => {
  const [rollingLog, setRollingLog] = useState<string[]>([]);
  const [retrigger, setRetrigger] = useState<boolean>(false);
  const lok_port = 8000;
  let deployment = app.name;

  const { run: logcommand, logs, error, finished } = useLazyCommand({});

  useEffect(() => {
    const timeout = setInterval(() => {
      logcommand({
        program: "docker",
        args: service
          ? ["compose", "logs", "-f", "--tail", "50", service]
          : ["compose", "logs", "-f", "--tail", "50"],
        options: {
          cwd: app.path,
        },
      });
    }, 1000);
    return () => clearInterval(timeout);
  }, [retrigger]);

  return (
    <div className="h-full w-full relative">
      <div className="text-xl flex flex-row bg-back-800 text-white shadow-xl mb-2 p-2 ">
        <div className="flex-1 my-auto ">
          <Link to={`/dashboard/${app.name}`}>{"< Back"}</Link>
        </div>
        <div className="flex-grow my-auto text-center">
          {app.name} - {service}
        </div>
        <div className="flex-1 my-auto text-right">
          <Link to={`/logs/${app.name}`}>Logs</Link>
        </div>
      </div>
      <div className="font-light mt-2">Log on</div>
      <pre>
        {logs.map((l, index) => (
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
  const { id, service } = useParams<{ id: string; service: string }>();
  const { apps } = useStorage();

  let app = apps.find((app) => app.name === id);

  return app ? (
    <Logs app={app} service={service} />
  ) : (
    <>Could not find this app</>
  );
};
