import { invoke } from "@tauri-apps/api";
import { useEffect, useState } from "react";
import { BindingsContext, Binding } from "./context";

export const BindingsProvider = ({
  children,
}: {
  children: React.ReactNode;
}) => {
  const [bindings, setBindings] = useState<Binding[]>([]);

  useEffect(() => {
    console.log("AdverstisedHostsForm");
    invoke("list_network_interfaces", { v4: true })
      .then((res) => {
        console.log(res);
        let x = (res as Binding[]).reduce(
          (prev, curr) =>
            prev.find((b) => b.host == curr.host) ? prev : [...prev, curr],
          [] as Binding[]
        );

        setBindings(x);
      })
      .catch((err) => console.error(err));
  }, []);

  return (
    <BindingsContext.Provider
      value={{
        bindings,
      }}
    >
      {children}
    </BindingsContext.Provider>
  );
};
