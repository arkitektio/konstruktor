import { A } from "@tauri-apps/api/cli-3e179c0b";
import { useField, useFormik, useFormikContext } from "formik";
import React, { useEffect } from "react";
import { Hover } from "../../../layout/Hover";
import { SetupValues } from "../Setup";
import napari from "../../../assets/logo-napari.png";
import fiji from "../../../assets/logo-fiji.png";
import mikrom from "../../../assets/logo-mikromanager.png";
import arkitektm from "../../../assets/logo-arkitekt.png";

export type App = {
  name: string;
  long: string;
  description: string;
  identifier: string;
  version: string;
  scopes: string[];
  image: string;
  requires: string[];
  client_type: "public" | "confidential";
  grant_type: "authorization-code" | "client-credentials";
  experimental?: boolean;
  download?: string;
  client_id: string;
  client_secret: string;
  redirect_uris?: string[];
};

export const available_apps: App[] = [
  {
    name: "orkestrator",
    identifier: "github.io.jhnnsrs.orkestrator",
    version: "latest",
    description: "The interface",
    long: "Orkestrator is the default interface for the platform, enabling you to visualize and control your data and apps",
    image:
      "https://cdn.sstatic.net/Img/teams/teams-illo-free-sidebar-promo.svg?v=47faa659a05e",
    requires: ["lok", "rekuest", "mikro"],
    client_id: "snfoinosinefsef",
    client_secret: "9noinfpuisenfpsiuenfpiosenfiusef",
    redirect_uris: [
      "http://localhost:8090",
      "http://localhost:6789/callback",
      "http://localhost:6789",
    ],
    client_type: "public",
    grant_type: "authorization-code",
    scopes: ["read", "write"],
  },
  {
    name: "doks",
    identifier: "github.io.jhnnsrs.doks",
    version: "latest",
    description: "The documentation",
    long: "This allows you to play around with your own data on the developer documentation. This app will not be able to modify your data",
    image:
      "https://cdn.sstatic.net/Img/teams/teams-illo-free-sidebar-promo.svg?v=47faa659a05e",
    requires: ["lok", "mikro"],
    client_id: "soinfosienfsfosefghsegfisnefoisneofinsef",
    client_secret: "soinfoefsefssdfienfoisnefoisneofinsef",
    redirect_uris: ["http://localhost:8090"],
    client_type: "public",
    grant_type: "authorization-code",
    scopes: ["read", "write"],
  },
  {
    name: "MikroJ",
    identifier: "github.io.jhnnsrs.mikroj",
    version: "latest",
    description: "The Workhorse",
    long: "Enables support for ImageJ and its macros",
    image:
      "https://cdn.sstatic.net/Img/teams/teams-illo-free-sidebar-promo.svg?v=47faa659a05e",
    requires: ["lok", "rekuest", "mikro"],
    download: "https://github.com/jhnnsrs/mikroj",
    client_id: "soinfosienfoisnseghsggegefoisneofinsef",
    client_secret: "soinfosienesfseffoisnefoisneofinsef",
    redirect_uris: ["http://localhost:8090"],
    client_type: "public",
    grant_type: "authorization-code",
    scopes: ["read", "write"],
  },
  {
    name: "MikroManager",
    identifier: "github.io.jhnnsrs.mikromanager",
    version: "latest",
    description: "The mikroscope",
    long: "Enables support for Micro manager, a microscope control software",
    image:
      "https://cdn.sstatic.net/Img/teams/teams-illo-free-sidebar-promo.svg?v=47faa659a05e",
    requires: ["lok", "rekuest", "mikro"],
    download: "https://github.com/jhnnsrs/mikroj",
    client_id: "soinfosienfoaswdasdasdisnefoisneofinsef",
    client_secret: "soinfosiengeesegegfoisnefoisneofinsef",
    redirect_uris: ["http://localhost:8090"],
    client_type: "public",
    grant_type: "authorization-code",
    scopes: ["read", "write"],
  },
  {
    name: "napari",
    identifier: "github.io.jhnnsrs.mikro-napari",
    version: "latest",
    description: "The viewer",
    long: "Napari is a python based image viewer that is used by many bioimage researchers",
    image: "http://localhost:8090/static/images/arkitekt.png",
    requires: ["lok", "rekuest"],
    download: "https://github.com/jhnnsrs/mikro-napari",
    client_id: "soinfosienfoissgsegsegtbsynefoisneofinsef",
    client_secret: "soinfosienfoissdfsdfnefoisneofinsef",
    redirect_uris: ["http://localhost:8090"],
    client_type: "public",
    grant_type: "authorization-code",
    scopes: ["read", "write"],
  },
];

const logoForApp = (name: string) => {
  switch (name) {
    case "MikroJ":
      return fiji;
    case "MikroManager":
      return mikrom;
    case "napari":
      return napari;
    default:
      return arkitektm;
  }
};

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
      <Hover className="grid grid-cols-3 @xl:grid-cols-4 gap-2 ">
        {available_apps.map((app, i) => {
          let disabled = app.requires.some(
            (r) => !values?.services?.find((s) => s.name === r)
          );

          return (
            <button
              className={` @container overflow-hidden hovercard group relative disabled:opacity-20 bg-slate-800 disabled:border-slate-200 border items-start flex rounded cursor-pointer  ${
                field.value && field.value.find((i: App) => i.name === app.name)
                  ? " border-slate-200 "
                  : "shadow-primary-300/20 border-slate-400 opacity-40 "
              }`}
              key={i}
              onClick={() => toggleValue(app)}
              disabled={disabled}
            >
              <div className="items-start p-3">
                <div className="flex flex-row justify-between">
                  <img
                    className="text-sm text-start h-20"
                    src={logoForApp(app.name)}
                  />
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
                      <div className="text-sm">{app.requires.join(", ")}</div>
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
