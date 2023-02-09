import React, { useEffect } from "react";
import { CommunicationContext } from "./communication-context";
import { invoke } from "@tauri-apps/api/tauri";
export type ICommunicationProviderProps = {
  children: React.ReactNode;
};

const CommunicationProvider: React.FC<ICommunicationProviderProps> = ({
  children,
}) => {
  function call(channel: string, options?: any) {
    return invoke(channel, {
      event: JSON.stringify(options || {}),
    }).then((res: unknown) => JSON.parse(res as string));
  }

  return (
    <CommunicationContext.Provider value={{ call: call }}>
      {children}
    </CommunicationContext.Provider>
  );
};

export { CommunicationProvider };
