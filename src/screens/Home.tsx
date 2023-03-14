import { Link } from "react-router-dom";

import { forage } from "@tauri-apps/tauri-forage";
import { useEffect, useState } from "react";
import { useStorage } from "../storage/storage-context";
import { Logo } from "../layout/Logo";
import { Hover } from "../layout/Hover";

export const Home: React.FC<{}> = (props) => {
  const { apps, deleteApp, deleteAllApps } = useStorage();

  return (
    <div className="w-full p-6">
      <div className="flex flex-col items-center">
        <div className="text-center">Welcome to</div>
        <Logo
          width={"20rem"}
          height={"20rem"}
          cubeColor={"rgb(var(--color-primary-300))"}
          aColor={"var(--color-back-700)"}
          strokeColor={"var(--color-back-700)"}
        />
        <div className="text-center text-5xl font-light mb-6">Arkitekt</div>
        <div>
          {apps.length > 0 ? (
            <div className="flex flex-col items-center">
              <div className="font-light text-center w-full mt-2">
                Available apps
              </div>
              <Hover className="flex flex-wrap col flex-row gap-2 mt-2">
                {apps.map((app, index) => (
                  <div
                    className="hovercard border rounded border-gray-300 p-5"
                    key={index}
                  >
                    <Link to={`/dashboard/${app.name}`}>{app.name}</Link>
                  </div>
                ))}
              </Hover>
            </div>
          ) : (
            <div className="text-center">
              Seems like this is your first time setting things app!
            </div>
          )}
        </div>
        <Hover className="flex flex-row items-center gap-2">
          <Link
            to="/setup"
            className="border rounded p-3 border-gray-400 font-light mt-8"
          >
            Setup new App
          </Link>
        </Hover>
      </div>
    </div>
  );
};
