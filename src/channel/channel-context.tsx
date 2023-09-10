import React, { useContext } from "react";
import { Channel } from "../repo/repo-context";

export const ChannelContext = React.createContext<Channel>({
  title: "Loading",
  name: "Loading",
  builder: "Loading",
  logo: "Loading",
  long: "Loading",
  experimental: false,
  description: "Loading",
  features: [],
  forms: [],
  defaults: {},
  availableApps: [],
  availableServices: [],
});

export const useChannel = () => useContext(ChannelContext);
