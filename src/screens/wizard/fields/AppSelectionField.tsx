import { FieldProps, useField, useFormikContext } from "formik";
import { Hover } from "../../../layout/Hover";
import { App, SetupValues } from "../../../repo/repo-context";
import { useChannel } from "../../../channel/channel-context";

export const AppSelectionField = ({ ...props }: any) => {
  const [field, meta, helpers] = useField<string[]>(props);
  const { values } = useFormikContext<SetupValues>();
  const { availableApps } = useChannel();

  const toggleValue = async (app: App) => {
    if (field.value) {
      if (field.value.find((i) => i === app.name)) {
        helpers.setValue(field.value.filter((i) => i !== app.name));
      } else {
        helpers.setValue([...field.value, app.name]);
      }
    } else {
      helpers.setValue([app.name]);
    }
    console.log(field.value);
  };

  return (
    <>
      <Hover className="grid grid-cols-3 @xl:grid-cols-4 gap-2 ">
        {availableApps.map((app, i) => {
          let disabled = app.requires?.some(
            (r) => !values?.services?.find((s) => s.name === r)
          );

          return (
            <button
              className={` @container overflow-hidden hovercard group relative disabled:opacity-20 bg-back-800 disabled:border-slate-200 border items-start flex rounded cursor-pointer  ${
                field.value && field.value.find((i) => i === app.name)
                  ? " border-slate-200 "
                  : "shadow-primary-300/20 border-slate-400 opacity-40 "
              }`}
              key={i}
              onClick={() => toggleValue(app)}
              disabled={disabled}
            >
              <div className="items-start p-3">
                <div className="flex flex-row justify-between">
                  {app.logo && (
                    <img className="text-sm text-start h-20" src={app.logo} />
                  )}
                </div>
                <div className="font-bold text-start flex-row flex justify-between">
                  <div className="my-auto">{app.name}</div>
                  {app.experimental && (
                    <div className="text-xs border-red-300 border p-1 rounded rounded-md">
                      Exp
                    </div>
                  )}
                </div>
                <div className="font-light  text-start">{app.description}</div>
                <div className="text-sm  text-start mt-1">{app.long}</div>
              </div>
              {disabled && (
                <>
                  <div className="absolute bottom-0 inset-0 bg-gradient-to-t w-full from-black to-transparent opacity-0 group-hover:opacity-100 z-0 rounded rounded-md items-end flex flex-row">
                    <div className="text-white text-center w-full flex flex-col ">
                      <div className="text-white text-center w-full ">
                        Disabled because of missing services
                      </div>
                      <div className="text-sm">{app.requires?.join(", ")}</div>
                    </div>
                  </div>
                </>
              )}
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
