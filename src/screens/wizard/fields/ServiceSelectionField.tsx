import { useField } from "formik";
import React from "react";
import { Hover } from "../../../layout/Hover";
import logod from "../../../assets/logo-docker.png";
import jupyter from "../../../assets/logo-jupyter.png";
import minio from "../../../assets/logo-minio.png";
import postgres from "../../../assets/logo-postgres.png";
import redis from "../../../assets/logo-redis.png";
import rabbitmq from "../../../assets/logo-rabbitmq.png";
import openid from "../../../assets/logo-openid.png";
import arkitektm from "../../../assets/logo-arkitekt.png";

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
    long: "This allows services to publish and subscribe to events",
    image: "redis:latest",
    requires: [],
    is_backend: true,
    extras: {},
  },
  {
    name: "postgres",
    interface: "db",
    description: "The database",
    long: "Storing your meta data",
    image: "jhnnsrs/daten:prod",
    requires: [],
    is_backend: true,
    extras: {},
  },
  {
    name: "minio",
    interface: "minio",
    description: "The storage",
    long: "Storing your images and files",
    image: "minio/minio:RELEASE.2023-02-10T18-48-39Z",
    requires: [],
    is_backend: true,
    extras: {},
  },
  {
    name: "rabbitmq",
    interface: "rabbitmq",
    description: "The backbone",
    long: "Taking care of the reliable communication between the apps",
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
    extras: {},
  },
  {
    name: "hub",
    interface: "hub",
    description: "The hub",
    long: "Access this compuer resources from anywhere in nice juypter notebooks",
    image: "jhnnsrs/hub:prod",
    requires: ["lok"],
    extras: {},
  },
];

export const logoForService = (service: Service) => {
  switch (service.name) {
    case "redis":
      return redis;
    case "postgres":
      return postgres;
    case "minio":
      return minio;
    case "hub":
      return jupyter;
    case "rabbitmq":
      return rabbitmq;
    case "port":
      return logod;
    default:
      return arkitektm;
  }
};

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
      <Hover className="grid grid-cols-3 @xl:grid-cols-4 gap-2">
        {available_services.map((app, i) => (
          <div
            className={` @container hovercard cursor-pointer border border-1 bg-back-800 ${
              field.value &&
              field.value.find((i: Service) => i.name === app.name)
                ? " border-slate-200 "
                : "shadow-primary-300/20 border-slate-400 opacity-40 "
            }`}
            key={i}
            onClick={() => toggleValue(app)}
          >
            <div className="items-start p-3">
              <div className="flex flex-row justify-between">
                <img
                  className="text-sm text-start h-20"
                  src={logoForService(app)}
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
          </div>
        ))}
      </Hover>

      {meta.touched && meta.error ? (
        <div className="error">{meta.error}</div>
      ) : null}
    </>
  );
};
