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

  const { run: logcommand, logs, error, finished } = useLazyCommand({});


  const reload = () => {
    logcommand({
      program: "docker",
      args: service
        ? ["compose", "logs", "--tail", "30", service]
        : ["compose", "logs", "--tail", "30"],
      options: {
        cwd: app.path,
      },
    });
  }



  useEffect(() => {
    reload();
  }, []);

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
      <div className="flex-col">
      <Button className="relative top-0 left-0" onClick={() => reload()}>Reload</Button>
      <div className="flex-grow bg-card rounded rounded-xl p-2 relative overflow-y-scroll">
        <pre className="flex-grow bg-card rounded rounded-xl p-2 relative" >
          {logs && logs.length > 0 ? (
            logs.map((l, index) => (
              <div key={index}>
                {l}
                <br />
              </div>
            ))
          ) : (
            <>No logs</>
          )}
          
        </pre>
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
