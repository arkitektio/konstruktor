import { A } from "@tauri-apps/api/cli-3e179c0b";
import { useField, useFormik, useFormikContext } from "formik";
import React from "react";
import { SetupValues } from "../Setup";

export type App = {
  name: string;
  long: string;
  description: string;
  image: string;
  requires: string[];
  client_type: "public" | "confidential";
  grant_type: "authorization_code" | "client_credentials";
  experimental?: boolean;
  download?: string;
  client_id: string;
  client_secret: string;
  redirect_uris?: string[];
};

export const available_apps: App[] = [
  {
    name: "orkestrator",
    description: "The interface",
    long: "Orkestrator is the default interface for the platform, enabling you to visualize and control your data and apps",
    image:
      "https://cdn.sstatic.net/Img/teams/teams-illo-free-sidebar-promo.svg?v=47faa659a05e",
    requires: ["core", "rekuest", "mikro"],
    client_id: "soinfosienfoisnefoisneofinsef",
    client_secret: "soinfosienfoisnefoisneofinsef",
    redirect_uris: ["http://localhost:8090"],
    client_type: "public",
    grant_type: "authorization_code",
  },
  {
    name: "doks",
    description: "The documentation",
    long: "This allows you to play around with your own data on the developer documentation. This app will not be able to modify your data",
    image:
      "https://cdn.sstatic.net/Img/teams/teams-illo-free-sidebar-promo.svg?v=47faa659a05e",
    requires: ["core", "mikro"],
    client_id: "soinfosienfoisnefoisneofinsef",
    client_secret: "soinfosienfoisnefoisneofinsef",
    redirect_uris: ["http://localhost:8090"],
    client_type: "public",
    grant_type: "authorization_code",
  },
  {
    name: "MikroJ",
    description: "The Workhorse",
    long: "Enables support for ImageJ and its makros",
    image:
      "https://cdn.sstatic.net/Img/teams/teams-illo-free-sidebar-promo.svg?v=47faa659a05e",
    requires: ["core", "rekuest", "mikro"],
    download: "https://github.com/jhnnsrs/mikroj",
    client_id: "soinfosienfoisnefoisneofinsef",
    client_secret: "soinfosienfoisnefoisneofinsef",
    redirect_uris: ["http://localhost:8090"],
    client_type: "public",
    grant_type: "authorization_code",
  },
  {
    name: "MikroManager",
    description: "The mikroscope",
    long: "Enables support for Micro manager, a microscope control software",
    image:
      "https://cdn.sstatic.net/Img/teams/teams-illo-free-sidebar-promo.svg?v=47faa659a05e",
    requires: ["core", "rekuest", "mikro"],
    download: "https://github.com/jhnnsrs/mikroj",
    client_id: "soinfosienfoisnefoisneofinsef",
    client_secret: "soinfosienfoisnefoisneofinsef",
    redirect_uris: ["http://localhost:8090"],
    client_type: "public",
    grant_type: "authorization_code",
  },
  {
    name: "MikroManager",
    description: "The mikroscope",
    long: "Enables support for Micro manager, a microscope control software",
    image:
      "https://cdn.sstatic.net/Img/teams/teams-illo-free-sidebar-promo.svg?v=47faa659a05e",
    requires: ["core", "rekuest", "mikro"],
    download: "https://github.com/jhnnsrs/mikroj",
    client_id: "soinfosienfoisnefoisneofinsef",
    client_secret: "soinfosienfoisnefoisneofinsef",
    redirect_uris: ["http://localhost:8090"],
    client_type: "public",
    grant_type: "authorization_code",
  },
  {
    name: "napari",
    description: "The viewer",
    long: "Napari is a python based image viewer that is used by many bioimage researchers",
    image: "http://localhost:8090/static/images/arkitekt.png",
    requires: ["core", "rekuest"],
    download: "https://github.com/jhnnsrs/mikro-napari",
    client_id: "soinfosienfoisnefoisneofinsef",
    client_secret: "soinfosienfoisnefoisneofinsef",
    redirect_uris: ["http://localhost:8090"],
    client_type: "public",
    grant_type: "authorization_code",
  },
];

const is_required_by = (name: string, service: App) => {
  return available_apps
    .find((s) => s.name === name)
    ?.requires.some((r) => r === service.name);
};

export const AppSelectionField = ({ ...props }: any) => {
  const [field, meta, helpers] = useField<App[]>(props);
  const { values } = useFormikContext<SetupValues>();

  const toggleValue = async (app: App) => {
    if (field.value) {
      if (field.value.find((i) => i.name === app.name)) {
        helpers.setValue(field.value.filter((i: App) => i.name !== app.name));
      } else {
        helpers.setValue([...field.value, app]);
      }
    } else {
      helpers.setValue([app]);
    }
    console.log(field.value);
  };

  return (
    <>
      <div className="grid grid-cols-3 @xl:grid-cols-6 gap-2 ">
        {available_apps.map((app, i) => {
          let disabled = app.requires.some(
            (r) => !values?.services?.find((s) => s.name === r)
          );

          return (
            <button
              className={` @container relative disabled:text-gray-600 border rounded border-gray-400 cursor-pointer items-center justify-center p-6 ${
                field.value &&
                field.value.find((i: App) => i.name === app.name) &&
                "bg-primary-300 border-primary-400 border-2 shadow-xl shadow-primary-300/20"
              }`}
              key={i}
              onClick={() => toggleValue(app)}
              disabled={disabled}
            >
              <div className="flex flex-col items-center justify-center p-6 @xl:underline">
                <div className="font-bold text-center @xs:underline">
                  {app.name}
                </div>
                <div className="font-light text-center">{app.description}</div>
                <div className="text-sm">{app.long}</div>
                {app.experimental && (
                  <div className="text-sm mt-2 border-red-300 border p-1 rounded rounded-md">
                    Experimental
                  </div>
                )}
              </div>
              {disabled && (
                <>
                  <div className="absolute inset-0 bg-gray-700 opacity-0 hover:opacity-100 z-0">
                    <div className="flex flex-col absolute inset-0 flex items-center justify-center text-slate-200 z-20">
                      <div className=" text-center">
                        Disabled because of missing services
                      </div>
                      <div className="text-sm">{app.requires.join(", ")}</div>
                    </div>
                  </div>
                </>
              )}
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
