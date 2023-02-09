import { useField } from "formik";
import React from "react";

export type Service = {
  name: string;
  long: string;
  description: string;
  image: string;
  requires: string[];
  experimental?: boolean;
};

export const available_services: Service[] = [
  {
    name: "core",
    description: "The core",
    long: "This includes authorization, authentificaiton, config management, and more",
    image:
      "https://cdn.sstatic.net/Img/teams/teams-illo-free-sidebar-promo.svg?v=47faa659a05e",
    requires: [],
  },
  {
    name: "mikro",
    description: "The datalayer",
    long: "Enables you to store, organize and monitor microscopy data",
    image:
      "https://cdn.sstatic.net/Img/teams/teams-illo-free-sidebar-promo.svg?v=47faa659a05e",
    requires: ["core"],
  },
  {
    name: "rekuest",
    description: "The broker",
    long: "Allows you to call enabled bioimage apps from the platform",
    image: "http://localhost:8090/static/images/arkitekt.png",
    requires: ["core"],
  },
  {
    name: "fluss",
    description: "The designer",
    long: "Allows you to design universal workflows spanning multiple apps",
    image: "http://localhost:8090/static/images/arkitekt.png",
    requires: ["core", "rekuest"],
  },
  {
    name: "port",
    description: "The virtualizer",
    long: "Enables one click install of github repos as internal apps",
    image: "http://localhost:8090/static/images/arkitekt.png",
    requires: ["core", "rekuest"],
    experimental: true,
  },
  {
    name: "hub",
    description: "The hub",
    long: "Access this compuer resources from anywhere in nice juypter notebooks",
    image: "http://localhost:8090/static/images/arkitekt.png",
    requires: ["core"],
    experimental: true,
  },
  {
    name: "vscode",
    description: "The code",
    long: "Access your computer resources and data from anywhere in nice vscode environemnts (uses third party)",
    image: "https://logowik.com/content/uploads/images/coder1889.jpg",
    requires: ["core"],
    experimental: true,
  },
];

const is_required_by = (name: string, service: Service) => {
  return available_services
    .find((s) => s.name === name)
    ?.requires.some((r) => r === service.name);
};

export const ServiceSelectionField = ({ ...props }: any) => {
  const [field, meta, helpers] = useField<Service[]>(props);

  const toggleValue = async (service: Service) => {
    if (field.value) {
      if (field.value.find((i: Service) => i.name === service.name)) {
        helpers.setValue(
          field.value.filter(
            (i: Service) =>
              i.name !== service.name &&
              is_required_by(i.name, service) === false
          )
        );
      } else {
        helpers.setValue([
          ...field.value,
          service,
          ...available_services.filter((s) =>
            service.requires.includes(s.name)
          ),
        ]);
      }
    } else {
      helpers.setValue([service]);
    }
    console.log(field.value);
  };

  return (
    <>
      <div className="grid grid-cols-3 @xl:grid-cols-6 gap-2">
        {available_services.map((app, i) => (
          <div
            className={` @container  border rounded border-gray-400 cursor-pointer ${
              field.value &&
              field.value.find((i: Service) => i.name === app.name) &&
              "bg-primary-300 border-primary-400 border-2 shadow-xl shadow-primary-300/20"
            }`}
            key={i}
            onClick={() => toggleValue(app)}
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
          </div>
        ))}
      </div>

      {meta.touched && meta.error ? (
        <div className="error">{meta.error}</div>
      ) : null}
    </>
  );
};
