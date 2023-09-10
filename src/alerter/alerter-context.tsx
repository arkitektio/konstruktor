import React, { useContext } from "react";

export type AlertingError = {
  error: string;
  message: string;
  subtitle: string;
  causedBy?: Error;
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
