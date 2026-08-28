import { open } from "@tauri-apps/plugin-dialog";
import React, { useCallback, useEffect, useMemo, useState } from "react";

import * as api from "../api";
import type { DeploymentRecord } from "../api";
import { RegistryContext, RegistryContextType } from "./registry-context";

/**
 * Keeps a cached copy of the Rust registry, and refreshes it after anything that could
 * have changed it — including a hub the command line created while the app was open.
 */
export const RegistryProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [deployments, setDeployments] = useState<DeploymentRecord[]>([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setDeployments(await api.listDeployments());
    } catch (e) {
      console.error("Could not read the deployment registry", e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const pickFolder = useCallback(async (title?: string) => {
    const picked = await open({
      directory: true,
      multiple: false,
      title: title ?? "Choose a folder for this deployment",
    });
    if (typeof picked !== "string") return undefined;

    // Canonical, so the registry and compose's working-dir label always agree, and
    // granted, so the app may write there at all.
    const path = await api.canonicalizePath(picked).catch(() => picked);
    await api.allowDeploymentDir(path).catch(() => undefined);
    return path;
  }, []);

  const suggestFolder = useCallback(async () => {
    const suggested = await api.suggestFolder();
    if (!suggested) return undefined;
    // Created here rather than at submit time, so the folder step can verify it.
    const prepared = await api.prepareDeploymentDir(suggested).catch(() => undefined);
    return prepared?.path ?? suggested;
  }, []);

  const value = useMemo<RegistryContextType>(
    () => ({
      deployments,
      loading,
      pickFolder,
      suggestFolder,
      inspectFolder: api.inspectFolder,
      forget: async (id: string) => {
        await api.forgetDeployment(id);
        await refresh();
      },
      byId: (id: string) => deployments.find((d) => d.id === id),
      refresh,
    }),
    [deployments, loading, pickFolder, suggestFolder, refresh]
  );

  return (
    <RegistryContext.Provider value={value}>{children}</RegistryContext.Provider>
  );
};
