import { forage } from "@tauri-apps/tauri-forage";
import React, { useCallback, useEffect, useState } from "react";
import { InstalledApp } from "../screens/wizard/types";
import { AlerterContext, AlertingError } from "./alerter-context";
import { ErrorBoundary } from "react-error-boundary";

export const AlerterProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [activeError, setActiveError] = useState<AlertingError | null>(null);

  const catchAlert = useCallback(
    (e: Error | unknown) => {
      if (e instanceof Error) {
        setActiveError({
          error: e.name,
          message: e.message,
          subtitle: e.stack || "",
          causedBy: e,
        });
        return;
      } else {
        setActiveError({
          error: "Unknown Error",
          message: "An unknown error occurred",
          subtitle: JSON.stringify(e),
        });
      }
    },
    [setActiveError]
  );

  const alert = useCallback(
    (e: AlertingError) => {
      setActiveError(e);
    },
    [setActiveError]
  );

  const ack = useCallback(() => {
    setActiveError(null);
  }, [setActiveError]);

  return (
    <AlerterContext.Provider
      value={{
        catchAlert: catchAlert,
        alert: alert,
        activeError: activeError,
        ack: ack,
      }}
    >
      {children}
    </AlerterContext.Provider>
  );
};
