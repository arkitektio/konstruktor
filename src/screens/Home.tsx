import { Link } from "react-router-dom";

import { forage } from "@tauri-apps/tauri-forage";
import { useEffect, useState } from "react";
import { useStorage } from "../storage/storage-context";
import { Logo } from "../layout/Logo";

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
              <div className="font-light text-center w-full">
                Available apps
              </div>
              <div className="grid grid-cols-2 gap-2 p-3">
                {apps.map((app) => (
                  <div className="border rounded border-gray-300 p-5">
                    <Link to={`/dashboard/${app.name}`}>
                      Dashboard for {app.name}
                    </Link>
                  </div>
                ))}
              </div>
            </div>
          ) : (
            <div className="text-center">
              Seems like this is your first time setting things app!
            </div>
          )}
        </div>
        <Link
          to="/setup"
          className="border rounded p-3 border-gray-400 font-light mt-2"
        >
          Setup new App
        </Link>
        <button
          onClick={() => deleteAllApps()}
          className="border rounded p-3 border-gray-400 font-light mt-2"
        >
          Delete all apps
        </button>
      </div>
    </div>
  );
};
