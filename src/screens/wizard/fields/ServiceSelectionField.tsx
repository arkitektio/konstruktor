import { useField } from "formik";
import React from "react";

export type Service = {
  name: string;
  interface: string;
  long: string;
  description: string;
  image: string;
  requires: string[];
  experimental?: boolean;
  is_backend?: boolean;
  extras: {};
};

export const available_services: Service[] = [
  {
    name: "redis",
    interface: "redis",
    description: "The pubsub",
    long: "This allows you to publish and subscribe to events",
    image: "redis:latest",
    requires: [],
    is_backend: true,
    extras: {},
  },
  {
    name: "postgres",
    interface: "db",
    description: "The database",
    long: "Storing your structured data",
    image: "jhnnsrs/daten:prod",
    requires: [],
    is_backend: true,
    extras: {},
  },
  {
    name: "minio",
    interface: "minio",
    description: "The database",
    long: "Storing your minio data",
    image: "minio/minio:RELEASE.2023-02-10T18-48-39Z",
    requires: [],
    is_backend: true,
    extras: {},
  },
  {
    name: "rabbitmq",
    interface: "rabbitmq",
    description: "The backbone",
    long: "This allows you to publish and subscribe to events",
    image: "jhnnsrs/mister:fancy",
    requires: [],
    is_backend: true,
    extras: {},
  },
  {
    name: "lok",
    interface: "lok",
    description: "The core",
    long: "This includes authorization, authentificaiton, config management, and more",
    image: "jhnnsrs/lok:prod",
    requires: ["redis", "db", "minio"],
    extras: {},
  },
  {
    name: "mikro",
    interface: "mikro",
    description: "The datalayer",
    long: "Enables you to store, organize and monitor microscopy data",
    image: "jhnnsrs/mikro:prod",
    requires: ["redis", "lok", "db", "minio"],
    extras: {},
  },
  {
    name: "rekuest",
    interface: "rekuest",
    description: "The broker",
    long: "Allows you to call enabled bioimage apps from the platform",
    image: "jhnnsrs/rekuest:prod",
    requires: ["redis", "rabbitmq", "lok", "db"],
    extras: {},
  },

  {
    name: "fluss",
    interface: "fluss",
    description: "The designer",
    long: "Allows you to design universal workflows spanning multiple apps",
    image: "jhnnsrs/fluss:prod",
    requires: ["redis", "lok", "rekuest", "rabbitmq", "db", "minio"],
    extras: {},
  },
  {
    name: "port",
    interface: "port",
    description: "The virtualizer",
    long: "Enables one click install of github repos as internal apps",
    image: "jhnnsrs/port:prod",
    requires: ["redis", "lok", "rekuest", "rabbitmq", "db"],
    experimental: true,
    extras: {},
  },
  {
    name: "hub",
    interface: "hub",
    description: "The hub",
    long: "Access this compuer resources from anywhere in nice juypter notebooks",
    image: "jhnnsrs/hub:prod",
    requires: ["lok"],
    experimental: true,
    extras: {},
  },
];

const is_required_by = (interfacex: string, service: Service) => {
  return available_services
    .find((s) => s.interface === interfacex)
    ?.requires.some((r) => r === service.interface);
};

export const ServiceSelectionField = ({ ...props }: any) => {
  const [field, meta, helpers] = useField<Service[]>(props);

  const toggleValue = async (service: Service) => {
    if (field.value) {
      if (field.value.find((i: Service) => i.interface === service.interface)) {
        helpers.setValue(
          field.value.filter(
            (i: Service) =>
              i.interface !== service.interface &&
              is_required_by(i.interface, service) === false
          )
        );
      } else {
        helpers.setValue([
          ...field.value,
          service,
          ...available_services.filter((s) =>
            service.requires.includes(s.interface)
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
