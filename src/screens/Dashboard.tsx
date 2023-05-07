import { invoke } from "@tauri-apps/api";
import React, { useEffect, useState } from "react";
import { TbReload } from "react-icons/tb";
import { GrBeacon } from "react-icons/gr";
import { Link, useNavigate, useParams } from "react-router-dom";
import { useCommunication } from "../communication/communication-context";
import { ResponsiveGrid } from "../layout/ResponsiveGrid";
import { App, useStorage } from "../storage/storage-context";
import { BeaconInterface } from "../types";
import { SetupValues } from "./wizard/Setup";
import { useBeacon } from "../beacon/context";

import { Command, open } from "@tauri-apps/api/shell";
import { Hover } from "../layout/Hover";

export enum DockerConnectionStrategy {
  LOCAL = "LOCAL",
  REMOTE = "REMOTE",
}

export type DockerConfig = {
  strategy: DockerConnectionStrategy;
  addr?: string;
};

export type DockerStatus = {
  version: string;
  memory: number;
};

export const ServiceHealth = (props: { service: any }) => {
  return (
    <ResponsiveGrid>
      {props?.service?.ok &&
        Object.keys(props?.service?.ok).map((key) => (
          <div className="border rounded bg-green-200 border-green-600 bg-white p-3 shadow-xl">
            <div className="font-light">{key}</div>
            {JSON.stringify((props?.service?.ok as any)[key])}
          </div>
        ))}
      {props?.service?.error &&
        Object.keys(props?.service?.error).map((key) => (
          <div className="border rounded bg-red-200 border-red-600 bg-white p-3 shadow-xl">
            <div className="font-light">{key}</div>
            {JSON.stringify((props?.service?.error as any)[key])}
          </div>
        ))}
    </ResponsiveGrid>
  );
};

export type InitDirectoryValues = {
  dirpath: string;
};

export type Container = {
  id: string;
  names: string[];
  status: string;
  labels: { [key: string]: string };
  state: string;
};

export type ContainerQuery = {
  containers: Container[];
};

export type Service = {
  name: string;
  containers: Container[];
};

const getServiceColor = (service: Service) => {
  if (service.containers.length === 0) {
    return "bg-gray-200 border-gray-600";
  }
  if (service.containers.every((c) => c.state == "running")) {
    return "bg-green-400 border-green-600";
  }
  if (service.containers.some((c) => c.state == "running")) {
    return "bg-yellow-200 border-yellow-600";
  }

  return "bg-red-200 border-red-600";
};

const getContainerColor = (container: Container) => {
  if (container.state == "running") {
    return "border-green-600";
  }
  if (container.state == "exited") {
    return "border-red-600";
  }
};

import {
  readDir,
  BaseDirectory,
  FileEntry,
  writeTextFile,
  createDir,
  removeDir,
} from "@tauri-apps/api/fs";
import { CommandParams, useCommand } from "../hooks/useCommand";
import { InstalledApp } from "./wizard/types";
import { useSettings } from "../settings/settings-context";
import { CommandButton, DangerousCommandButton } from "../CommandButton";
import { Konstrukt } from "../Konstrukt";

export const Dashboard: React.FC<{ app: App }> = ({ app }) => {
  const { call } = useCommunication();
  const { deleteApp } = useStorage();
  const navigate = useNavigate();
  const [dockerStatus, setDockerStatus] = useState<DockerStatus | null>(null);
  const [advertise, setAdvertise] = useState<boolean>(false);
  const [services, setServices] = useState<Service[]>([]);
  const { advertisedSignals, toggleSignal } = useBeacon();
  const [retrigger, setRetrigger] = useState<boolean>(false);
  const { settings } = useSettings();
  const [initialized, setInitialized] = useState<boolean>(false);
  const [countDown, setCountDown] = useState<number>(4);

  const openFolder = async () => {
    await open(app.path);
    console.log("opened");
  };

  const {
    run: up,
    logs: uplog,
    error: uperror,
    finished: upfinished,
  } = useCommand({
    program: "docker",
    args: ["compose", "up", "-d"],
    options: {
      cwd: app.path,
    },
  });

  const pull = useCommand({
    program: "docker",
    args: ["compose", "pull"],
    options: {
      cwd: app.path,
    },
  });

  const { run: down } = useCommand({
    program: "docker",
    args: ["compose", "down"],
    options: {
      cwd: app.path,
    },
  });

  const lok_port = 8000;
  let deployment = app.name;
  const [bindings, setAvailableBindings] = useState<BeaconInterface[]>([]);

  const [restartingContainers, setRestartingContainers] = useState<string[]>(
    []
  );
  useEffect(() => {
    call<DockerConfig, DockerStatus>("test_docker", {
      strategy: DockerConnectionStrategy.LOCAL,
    }).then((res) => setDockerStatus(res));
  }, []);

  useEffect(() => {
    loadServiceState();
    let interval = setInterval(() => {
      loadServiceState();
    }, 2000);

    return () => clearInterval(interval);
  }, []);

  const processEntries = (entries: FileEntry[]) => {
    for (const entry of entries) {
      if (entry.name == "docker-compose.yaml") {
        setInitialized(true);
      }

      console.log(`Entry: ${entry.path}`);
      if (entry.children) {
        processEntries(entry.children);
      }
    }
  };

  const checkFiles = async () => {
    const entries = await readDir(`apps/${app.name}/`, {
      dir: BaseDirectory.App,
      recursive: true,
    });
    processEntries(entries);
  };

  useEffect(() => {
    checkFiles();
  }, [retrigger]);

  useEffect(() => {
    console.log("AdverstisedHostsForm");
    invoke("list_network_interfaces", { v4: true })
      .then((res) => {
        let x = (res as BeaconInterface[]).reduce(
          (prev, curr) =>
            prev.find((b) => b.host == curr.host) ? prev : [...prev, curr],
          [] as BeaconInterface[]
        );

        setAvailableBindings(x);
      })
      .catch((err) => console.error(err));
  }, []);

  const loadServiceState = () => {
    invoke<ContainerQuery>("nana_test", { deployment })
      .then((res) => {
        let services = res.containers.reduce((prev, curr) => {
          let service = curr.labels[`arkitekt.${deployment}.service`];
          if (service) {
            let x = prev.find((b) => b.name == service);
            if (x) {
              x.containers.push(curr);
            } else {
              prev.push({ name: service, containers: [curr] });
            }
          }
          return prev;
        }, [] as Service[]);

        console.log(res);
        setServices(services);
      })
      .catch((err) => console.log(err));
  };

  const restartContainer = (id: string) => {
    setRestartingContainers((prev) => [...prev, id]);
    invoke("restart_container", {
      containerId: id,
    })
      .then((res) =>
        setRestartingContainers((prev) => [...prev.filter((x) => x != id)])
      )
      .catch((err) => {
        setRestartingContainers((prev) => [...prev.filter((x) => x != id)]);
        alert(err);
      });
  };

  const toggleAdvertised = (inf: BeaconInterface) => {
    let url = `${inf.host}:${lok_port}`;
    toggleSignal({ url: url, bind: inf.bind, broadcast: inf.broadcast });
  };

  const is_advertised = (inf: BeaconInterface) => {
    let url = `${inf.host}:${lok_port}`;
    return !!advertisedSignals.find((x) => x.url == url);
  };

  const test_docker_version = () => {
    call("docker_version_cmd", {
      docker_addr: "ssss",
    }).then((res) => console.log(res));
  };

  return (
    <div className="h-full w-full flex flex-col">
      <div className="flex initial text-xl flex flex-row bg-back-800 text-white shadow-xl mb-2 p-2 ">
        <div className="flex-1 my-auto ">
          <Link to="/">{"< Home"}</Link>
        </div>
        <div className="flex-grow my-auto text-center">{app.name}</div>
        <div className="flex-1 my-auto text-right">
          <Link to={`/logs/${app.name}`}>Logs</Link>
        </div>
      </div>
      {!initialized ? (
        <div className="flex-grow flex flex-col h-full p-2 overflow-y-scroll items-center">
          <div className="font-bold text-3xl">Hello to</div>
          <div className="font-light text-5xl mt-2">{app.name}</div>
          <div className="text-9xl mt-2">☕</div>
          <div className="mt-5 max-w-xl align-center text-center">
            In order to use this app you need to initialize it with a builder. A
            buildr transforms your app into a startable containerized app and
            downloads all the necessary dependencies. For the modules that you
            want. In the future you will be able to choose between a few
            different builders. For now we only have one.
          </div>
          <div className="mt-5 flex flex-col text-3xl max-w-xl text-center items-center gap-2 mt-3 font-bold">
            This will take a while (be patient and/or go have a coffee).
          </div>

          <div className="mt-5 flex flex-col items-center gap-2">
            <Konstrukt app={app} callback={() => setRetrigger((y) => !y)} />
          </div>
        </div>
      ) : (
        <div className="flex-grow flex flex-col h-full p-2 overflow-y-scroll">
          <div className="flex flex-row gap-2 mb-3 gap-2">
            <div className="flex-grow flex flex-row flex-items-start  gap-2">
              <CommandButton
                params={{
                  program: "docker",
                  args: ["compose", "up"],
                  options: {
                    cwd: app.path,
                  },
                }}
                title="Start"
                runningTitle="Starting..."
              />
              <CommandButton
                params={{
                  program: "docker",
                  args: ["compose", "stop"],
                  options: {
                    cwd: app.path,
                  },
                }}
                title="Stop"
                runningTitle="Stopping..."
              />
            </div>
            <div className="flex-initial flex flex-row flex-items-start gap-2 ">
              <DangerousCommandButton
                params={{
                  program: "docker",
                  args: ["compose", "down"],
                  options: {
                    cwd: app.path,
                  },
                }}
                callback={() => alert("Torn down")}
                title="Tear down"
                runningTitle="Tearing down..."
              />
              <DangerousCommandButton
                params={{
                  program: "docker",
                  args: ["compose", "down"],
                  options: {
                    cwd: app.path,
                  },
                }}
                callback={() => deleteApp(app).then(() => navigate("/"))}
                title="Tear down and Delete"
                runningTitle="Tearing down..."
              />
              <DangerousCommandButton
                params={{
                  program: "docker",
                  args: ["compose", "pull"],
                  options: {
                    cwd: app.path,
                  },
                }}
                callback={() => alert("Updated")}
                title="Update"
                runningTitle="Updating..."
                to={5}
              />
            </div>
          </div>
          <div className="border-1 border-gray-300 rounded p-2 bg-white text-black">
            <div className="flex flex-row gap-2 justify-between">
              <div>
                <div className="flex flex-col gap-2">
                  <div className="font-bold">App</div>
                  <div className="grid grid-cols-2 gap-1">
                    <div onClick={() => openFolder()} className="font-light">
                      Path
                    </div>
                    <div
                      className="font-light cursor-pointer"
                      onClick={() => openFolder()}
                    >
                      {app.path}
                    </div>
                  </div>
                </div>
                <div className="font-bold">Docker</div>
                <div className="grid grid-cols-2 gap-1">
                  <div className="font-light">Version</div>
                  <div className="font-light">{dockerStatus?.version}</div>
                  <div className="font-light">Memory</div>
                  <div className="font-light">
                    {dockerStatus?.memory &&
                      (dockerStatus.memory / 1000000000).toPrecision(4)}{" "}
                    GB
                  </div>
                </div>
              </div>
              <div className="flex flex-col gap-2"></div>
            </div>
          </div>
          <div className="font-light mt-2">Status of Deployment</div>
          <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-6 xl:grid-cols-6 gap-2 mt-2">
            {services.map((s, index) => (
              <div
                key={index}
                className={`shadow shadow-lg border border-1 white p-2 p-3  text-slate-800 bg-slate-100 rounded rounded-md `}
              >
                <div className="flex flex-row justify-between">
                  <Link
                    to={`/logs/${app.name}/service/${s.name}`}
                    className="font-bold text-"
                  >
                    {s.name}
                  </Link>
                  <div
                    className={`h-3 w-3 rounded-full border ${getServiceColor(
                      s
                    )} inline-block`}
                  ></div>
                </div>
                <div className="flex flex-col gap-2">
                  {s.containers.map((c, index) => {
                    return (
                      <div
                        className={`group border border-1 white p-2 p-3  text-black rounded rounded-md ${getContainerColor(
                          c
                        )}`}
                      >
                        <div className="flex flex-row justify-between">
                          <div>
                            <div className="font-bold">
                              Instance {index + 1}
                            </div>
                            <div className="text-xs">{c.status}</div>
                          </div>
                          <button
                            className=" disabled:opacity-50"
                            onClick={() => restartContainer(c.id)}
                            disabled={restartingContainers.includes(c.id)}
                            title="Restart this container"
                          >
                            {restartingContainers.includes(c.id) ? (
                              "Restarting"
                            ) : (
                              <TbReload className="group-hover:visible invisible" />
                            )}
                          </button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            ))}
          </div>
          <div className="flex-initial"></div>
          <div className="font-light mt-2">Advertising on</div>
          <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-6 xl:grid-cols-6 gap-2 mt-1">
            {bindings
              .filter((c) => c.broadcast)
              .map((c, index) => (
                <div
                  key={index}
                  className={`group border border-1 border-slate-400  p-1 rounded text-slate-700 cursor-pointer ${
                    is_advertised(c) ? "bg-slate-100" : "bg-zinc-400 opacity-20"
                  } ${is_advertised(c) && advertise ? "" : ""}`}
                  onClick={() => toggleAdvertised(c)}
                >
                  <div className="flex flex-row justify-between">
                    <div className="text-xl truncate"> {c.host}</div>
                  </div>
                  <div className=" truncate">
                    {is_advertised(c) ? (
                      <div className="flex flex-row text-s">
                        <div className="my-auto">
                          <GrBeacon className="bg-slate-100" />
                        </div>
                        <div className="my-auto ml-1">{c.broadcast}</div>
                      </div>
                    ) : (
                      <div className="text-s group-hover:visible invisible">
                        {c.broadcast}
                      </div>
                    )}
                  </div>
                </div>
              ))}
          </div>

          <Hover className="absolute bottom-0 right-0 flex-1 flex flex-row justify-end gap-2">
            <div></div>
          </Hover>
        </div>
      )}
    </div>
  );
};

export const DashboardScreen: React.FC<{}> = (props) => {
  const { id } = useParams<{ id: string }>();
  const { apps } = useStorage();

  let app = apps.find((app) => app.name === id);

  return app ? <Dashboard app={app} /> : <>Could not find this app</>;
};
