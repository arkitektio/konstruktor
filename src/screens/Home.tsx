import { Link } from "react-router-dom";

import { forage } from "@tauri-apps/tauri-forage";
import { useEffect, useState } from "react";
import { useStorage } from "../storage/storage-context";
import { Logo } from "../layout/Logo";
import { Hover } from "../layout/Hover";

export const Home: React.FC<{}> = (props) => {
  const { apps, deleteApp } = useStorage();

  return (
    <div className="w-full h-full p-6 flex flex-col items-center">
      <div className="flex-initial text-center font-bold">
        <Logo
          width={"10rem%"}
          height={"10rem"}
          cubeColor={"rgb(var(--color-primary-400))"}
          aColor={"var(--color-back-700)"}
          strokeColor={"var(--color-back-700)"}
        />
      </div>
      <div className="flex-initial text-center font-bold text-4xl">
        Arkitekt
      </div>
      <div className="flex-grow"></div>
      <div className="flex-initial">
        {apps.length > 0 ? (
          <div className="flex flex-col items-center">
            <div className="font-light text-center w-full mt-2">
              Your deployments:
            </div>
            <Hover className="flex flex-wrap col flex-row gap-2 mt-2">
              {apps.map((app, index) => (
                <div
                  className="hovercard border rounded border-gray-300 p-2"
                  key={index}
                >
                  <Link to={`/dashboard/${app.name}`}>{app.name}</Link>
                </div>
              ))}
            </Hover>
          </div>
        ) : (
          <div className="text-center">
            Seems like this is your first time setting things up!
          </div>
        )}
      </div>
      <div className="flex-initial flex flex-row gap-2 ">
        <Hover className="flex flex-row items-center gap-2">
          <Link
            to="/setup"
            className="border rounded p-2 border-gray-400 font-light mt-8"
          >
            Setup new App
          </Link>
        </Hover>
        <Hover className="flex flex-row items-center gap-2">
          <Link
            to="/settings"
            className="border rounded p-2 border-gray-400 font-light mt-8"
          >
            Settings
          </Link>
        </Hover>
      </div>
    </div>
  );
};
