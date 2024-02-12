import { forage } from "@tauri-apps/tauri-forage";
import React, { useCallback, useEffect, useState } from "react";
import { InstalledApp } from "../screens/wizard/types";
import { RepoContext, Channel, repoSchema, RepoError } from "./repo-context";
import { ErrorBoundary } from "react-error-boundary";
import { ValidationError } from "yup";

export const RepoProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [channels, setActiveChannels] = useState<Channel[]>([]);
  const [errors, setErrorRepos] = useState<RepoError[]>([]);






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


  const validate = useCallback(
    (repoResult: any) => {
      try {
        console.log(repoResult)
        let result = repoSchema.parse(repoResult);
        ensureChannels(result.channels);
      }
      catch (e) {
        if (e instanceof ValidationError) {
          setErrorRepos(errors => [...errors, {repo: repoResult.repo || "unknown", errors: (e as ValidationError).inner}]);
        }
        console.error(e);
      }
    },
    [setErrorRepos, ensureChannels]
  );

  return (
    <RepoContext.Provider
      value={{
        channels,
        errors,
        ensureChannels,
        validate
      }}
    >
      {children}
    </RepoContext.Provider>
  );
};
