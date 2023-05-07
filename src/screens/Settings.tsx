import { invoke } from "@tauri-apps/api";
import React, { useEffect, useState } from "react";
import { TbReload } from "react-icons/tb";
import { GrBeacon } from "react-icons/gr";
import { Link, useNavigate, useParams } from "react-router-dom";
import { useCommunication } from "../communication/communication-context";
import { ResponsiveGrid } from "../layout/ResponsiveGrid";
import { useSettings } from "../settings/settings-context";
import { BeaconInterface } from "../types";
import { SetupValues } from "./wizard/Setup";
import { useBeacon } from "../beacon/context";

import { Command } from "@tauri-apps/api/shell";
import { Hover } from "../layout/Hover";
import { useCommand } from "../hooks/useCommand";
import { CommandButton } from "../CommandButton";

export const Settings = () => {
  const { settings } = useSettings();
  const {
    run: up,
    logs,
    error,
    finished,
  } = useCommand({
    program: "docker",
    args: ["pull", settings.baker],
  });

  return (
    <div className="h-full w-full relative">
      <div className="text-xl flex flex-row bg-back-800 text-white shadow-xl mb-2 p-2 ">
        <div className="flex-1 my-auto ">
          <Link to="/">{"< Home"}</Link>
        </div>
        <div className="flex-grow my-auto text-center">Settings</div>
      </div>
      <div className="flex flex-col h-full p-2  overflow-y-scroll">
        <div className="border-1 border-gray-300 rounded p-2 bg-white text-black">
          <div className="flex flex-row gap-2 justify-between">
            <div className="flex flex-col gap-2">
              <CommandButton
                title="Update Builder"
                params={{ program: "docker", args: ["pull", settings.baker] }}
                runningTitle="Pulling..."
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};
