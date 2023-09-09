import React, { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { useStorage, App } from "../storage/storage-context";
import { SetupValues } from "./wizard/Setup";

import { Command } from "@tauri-apps/api/shell";
import { useCommand, useLazyCommand } from "../hooks/useCommand";
import { Page } from "../layout/Page";
import { Button } from "../components/ui/button";

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
    <Page>
      
      <div className="flex-grow flex flex-col gap-2 p-3 ">
      <pre className="flex-grow bg-card rounded rounded-xl p-2">
        {logs && logs.length > 0 ? logs.map((l, index) => (
          <div key={index}>
            {l}
            <br />
          </div>
        )): <>No logs</>}
      </pre>
      </div>
      <div className="flex-initial flex flex-row gap-2 p-3 bg-card  border-t border-foreground">
        <div  className="flex flex-row items-center gap-2">
        <Button>
          <Link
            to="/"
          >
             <Link to={`/dashboard/${app.name}`}>{"< Back"}</Link>
          </Link>
          </Button>
        </div>
      </div>
    </Page>
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
