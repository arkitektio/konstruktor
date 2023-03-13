import { invoke } from "@tauri-apps/api";
import { A } from "@tauri-apps/api/cli-3e179c0b";
import {
  FieldHookConfig,
  FieldProps,
  useField,
  useFormik,
  useFormikContext,
} from "formik";
import React, { useEffect } from "react";
import { SetupValues } from "../Setup";

export type Binding = {
  name: string;
  host: string;
  broadcast: string;
};

export const HostSelectionField = ({ ...props }: { name: string }) => {
  const [field, meta, helpers] = useField(props.name);
  const { values } = useFormikContext<SetupValues>();
  const [availableBindings, setAvailableBindings] = React.useState<Binding[]>(
    []
  );

  useEffect(() => {
    console.log("AdverstisedHostsForm");
    invoke("list_network_interfaces", { v4: true })
      .then((res) => {
        let x = (res as Binding[]).reduce(
          (prev, curr) =>
            prev.find((b) => b.host == curr.host) ? prev : [...prev, curr],
          [] as Binding[]
        );

        setAvailableBindings(x);
      })
      .catch((err) => console.error(err));
  }, []);

  const toggleValue = async (binding: Binding) => {
    if (field.value) {
      if (field.value.find((i: Binding) => i.name === binding.name)) {
        helpers.setValue(
          field.value.filter((i: Binding) => i.name !== binding.name)
        );
      } else {
        helpers.setValue([...field.value, binding]);
      }
    } else {
      helpers.setValue([binding]);
    }
    console.log(field.value);
  };

  return (
    <>
      <div className="grid grid-cols-3 @xl:grid-cols-6 gap-2 ">
        {availableBindings.map((binding, i) => {
          return (
            <button
              className={` @container relative disabled:text-gray-600 border rounded border-gray-400 cursor-pointer items-center justify-center p-6 ${
                field.value &&
                field.value.find((i: Binding) => i.host === binding.host) &&
                "bg-primary-300 border-primary-400 border-2 shadow-xl shadow-primary-300/20"
              }`}
              key={i}
              onClick={() => toggleValue(binding)}
            >
              <div className="flex flex-col items-center justify-center p-6 @xl:underline">
                <div className="font-bold text-center @xs:underline">
                  {binding.host}
                </div>
                <div className="font-light text-xs text-center">
                  Network interface
                </div>
                <div className="font-light text-sm text-center">
                  {binding.name}
                </div>
              </div>
            </button>
          );
        })}
      </div>

      {meta.touched && meta.error ? (
        <div className="error">{meta.error}</div>
      ) : null}
    </>
  );
};
