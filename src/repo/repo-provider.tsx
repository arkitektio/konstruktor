import { forage } from "@tauri-apps/tauri-forage";
import React, { useCallback, useEffect, useState } from "react";
import { InstalledApp } from "../screens/wizard/types";
import { RepoContext, Channel } from "./repo-context";
import { ErrorBoundary } from "react-error-boundary";

export const RepoProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [channels, setActiveChannels] = useState<Channel[]>([]);

  const ensureChannels = useCallback(
    (channels: Channel[]) => {
      setActiveChannels((oldchannels) => {
        let newChannels = channels;

        oldchannels.forEach((channel) => {
          if (
            newChannels.find(
              (newChannel) => newChannel.name === channel.name
            ) === undefined
          ) {
            newChannels.push(channel);
          }
        });
        return newChannels;
      });
    },
    [setActiveChannels]
  );

  return (
    <RepoContext.Provider
      value={{
        channels,
        ensureChannels,
      }}
    >
      {children}
    </RepoContext.Provider>
  );
};
