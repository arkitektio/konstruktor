import React, { useContext } from "react";

export type Binding = {
  name: string;
  host: string;
  broadcast: string;
  successfull_dns: boolean;
};

export interface BindingContextType {
  bindings: Binding[];
}

export const BindingsContext = React.createContext<BindingContextType>({
  bindings: [],
});

export const useBindings = () => useContext(BindingsContext);
