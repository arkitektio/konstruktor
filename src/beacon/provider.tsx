import { invoke } from "@tauri-apps/api";
import { useEffect, useState } from "react";
import { BeaconContext } from "./context";

export type BeaconSignal = {
  bind: string;
  broadcast: string;
  url: string;
};

export const BeaconProvider = ({ children }: { children: React.ReactNode }) => {
  const [advertisedSignals, setAdvertisedSignals] = useState<BeaconSignal[]>(
    []
  );

  const toggleSignal = (signal: BeaconSignal) => {
    if (advertisedSignals.find((x) => x.url == signal.url)) {
      setAdvertisedSignals((prev) => prev.filter((x) => x.url != signal.url));
    } else {
      setAdvertisedSignals((prev) => [...prev, signal]);
    }
  };

  const advertiseEndpoint = () => {
    if (advertisedSignals) {
      invoke("advertise_endpoint", {
        signals: advertisedSignals,
      })
        .then((res) => console.log(res))
        .catch((err) => console.error(err));
    }
  };

  useEffect(() => {
    if (advertisedSignals.length > 0) {
      advertiseEndpoint();
      const interval = setInterval(() => {
        advertiseEndpoint();
      }, 2000 || 3000);
      return () => clearInterval(interval);
    }
    return () => {};
  }, [advertisedSignals]);

  return (
    <BeaconContext.Provider
      value={{
        advertisedSignals,
        toggleSignal,
      }}
    >
      {children}
    </BeaconContext.Provider>
  );
};
