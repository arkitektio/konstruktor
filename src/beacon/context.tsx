import React, { useContext } from "react";

export type BeaconSignal = {
  bind: string;
  broadcast: string;
  url: string;
};

export interface BeaconContextType {
  advertisedSignals: BeaconSignal[];
  toggleSignal: (signal: BeaconSignal) => void;
}

export const BeaconContext = React.createContext<BeaconContextType>({
  toggleSignal: (signal: BeaconSignal) => {},
  advertisedSignals: [],
});

export const useBeacon = () => useContext(BeaconContext);
