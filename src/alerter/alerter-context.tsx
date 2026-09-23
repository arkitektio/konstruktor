import React, { useContext } from "react";

import type { LogEntry } from "../compose-log";

export type AlertingError = {
  error: string;
  message: string;
  subtitle: string;
  causedBy?: Error;
  /** The command's output, shown under the message for reading what happened. */
  log?: LogEntry[];
};

export type AlerterContext = {
  catchAlert: (e: Error) => void;
  alert: (e: AlertingError) => void;
  activeError: AlertingError | null;
  ack: () => void;
};

export const AlerterContext = React.createContext<AlerterContext>({
  catchAlert(e: Error) {
    alert(e.message);
  },
  alert(e: AlertingError) {
    alert(e.message);
  },
  activeError: null,
  ack() {},
});

export const useAlerter = () => useContext(AlerterContext);
