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
import { Binding, useBindings } from "../../../interface/context";
import { Hover } from "../../../layout/Hover";
import { SetupValues } from "../Setup";
import { GrBeacon } from "react-icons/gr";

export const HostSelectionField = ({
  ...props
}: {
  name: string;
  availableBindings: Binding[];
}) => {
  const [field, meta, helpers] = useField(props.name);
  const { bindings } = useBindings();
  const { values } = useFormikContext<SetupValues>();

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
      <Hover className="grid grid-cols-1 @lg:grid-cols-3 @xl:grid-cols-4 gap-2 ">
        {bindings.map((binding, i) => {
          return (
            <button
              className={` @container overflow-hidden hovercard group relative disabled:opacity-20 bg-slate-800 disabled:border-slate-200 border items-start flex rounded cursor-pointer  ${
                field.value &&
                field.value.find((i: Binding) => i.host === binding.host)
                  ? " border-slate-200 "
                  : "shadow-primary-300/20 border-slate-400 opacity-40 "
              }`}
              key={i}
              onClick={() => toggleValue(binding)}
            >
              <div className="items-start p-3 w-full">
                <div className="font-bold text-start flex-row flex justify-between">
                  <div className="my-auto">{binding.host}</div>
                  <GrBeacon
                    className={` ${
                      field.value &&
                      field.value.find((i: Binding) => i.host === binding.host)
                        ? "color-primary-500"
                        : "opacity-0"
                    }`}
                  />
                </div>
                <div className="font-light  text-start">{binding.name}</div>
              </div>
            </button>
          );
        })}
      </Hover>

      {meta.touched && meta.error ? (
        <div className="error">{meta.error}</div>
      ) : null}
    </>
  );
};
