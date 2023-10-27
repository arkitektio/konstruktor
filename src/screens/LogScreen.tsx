import React, { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { App, useStorage } from "../storage/storage-context";

import { Button } from "../components/ui/button";
import { useLazyCommand } from "../hooks/useCommand";
import { Page } from "../layout/Page";

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
    logcommand({
      program: "docker",
      args: service
        ? ["compose", "logs", "-f", "--tail", "30", service]
        : ["compose", "logs", "-f", "--tail", "30"],
      options: {
        cwd: app.path,
      },
    });
    const timeout = setInterval(() => {
      logcommand({
        program: "docker",
        args: service
          ? ["compose", "logs", "-f", "--tail", "30", service]
          : ["compose", "logs", "-f", "--tail", "30"],
        options: {
          cwd: app.path,
        },
      });
    }, 4000);
    return () => clearInterval(timeout);
  }, [retrigger]);

  return (
    <Page
      buttons={
        <>
          <Button asChild>
            <Link to="/">
              <Link to={`/dashboard/${app.name}`}>{"< Back"}</Link>
            </Link>
          </Button>
        </>
      }
    >
      <pre className="flex-grow bg-card rounded rounded-xl p-2">
        {logs && logs.length > 0 ? (
          logs.toReversed().map((l, index) => (
            <div key={index}>
              {l}
              <br />
            </div>
          ))
        ) : (
          <>No logs</>
        )}
      </pre>
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
