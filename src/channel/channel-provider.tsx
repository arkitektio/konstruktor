import React from "react";
import { Channel } from "../repo/repo-context";
import { ChannelContext } from "./channel-context";

export const ChannelProvider: React.FC<{
  children: React.ReactNode;
  channel: Channel;
}> = ({ children, channel }) => {
  return (
    <ChannelContext.Provider
      value={{
        ...channel,
      }}
    >
      {children}
    </ChannelContext.Provider>
  );
};
