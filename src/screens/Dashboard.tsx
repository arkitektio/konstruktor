import { invoke } from "@tauri-apps/api";
import React, { useEffect, useState } from "react";
import { TbReload } from "react-icons/tb";
import { GrBeacon } from "react-icons/gr";
import { Link, useParams } from "react-router-dom";
import { useCommunication } from "../communication/communication-context";
import { ResponsiveGrid } from "../layout/ResponsiveGrid";
import { useStorage } from "../storage/storage-context";
import { BeaconInterface } from "../types";
import { SetupValues } from "./wizard/Setup";
import { useBeacon } from "../beacon/context";

import { Command } from "@tauri-apps/api/shell";

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

export const Dashboard: React.FC<{ app: SetupValues }> = ({ app }) => {
  const { call } = useCommunication();
  const [dockerStatus, setDockerStatus] = useState<DockerStatus | null>(null);
  const [advertise, setAdvertise] = useState<boolean>(false);
  const [services, setServices] = useState<Service[]>([]);
  const { advertisedSignals, toggleSignal } = useBeacon();
  const [rollingLog, setRollingLog] = useState<string[]>([]);
  const lok_port = 8000;
  let deployment = "default";
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

  const log = (line: string) => {
    setRollingLog((prev) => {
      return [line, ...prev].slice(0, 50);
    });
  };

  const app_up = () => {
    runDocker(["compose", "up", "-d"]).then((child) => {
      log(`command spawned with PID ${child.pid}`);
    });
  };

  const runDocker = async (args: string[]) => {
    const command = new Command("docker", args, {
      cwd: app.app_path,
    });
    command.on("close", (data) => {
      log(`command finished with code ${data.code} and signal ${data.signal}`);
    });
    command.on("error", (error) => console.error(`command error: "${error}"`));
    command.stdout.on("data", (line) => log(`command stdout: "${line}"`));
    command.stderr.on("data", (line) => log(`command stderr: "${line}"`));

    let child = await command.execute();
    return child;
  };

  const app_stop = () => {
    runDocker(["compose", "stop"]).then((child) => {
      log(`command spawned with PID ${child.code}`);
    });
  };

  const app_pull = () => {
    runDocker(["compose", "pull"]).then((child) => {
      log(`command spawned with PID ${child.code}`);
    });
  };

  const app_down = () => {
    runDocker(["compose", "down"]).then((child) => {
      log(`command spawned with PID ${child.code}`);
    });
  };

  const app_logs = () => {
    runDocker(["compose", "logs"]).then((child) => {
      log(`command spawned with PID ${child.code}`);
    });
  };

  return (
    <div className="h-full w-full">
      <div className="text-xl flex flex-row bg-gray-800 text-white shadow-xl mb-2 p-2 ">
        <div className="flex-1 my-auto ">
          <Link to="/">{"< Home"}</Link>
        </div>
        <div className="flex-grow my-auto text-center">{app.name}</div>
        <div className="flex-1 flex flex-row justify-end gap-2">
          <button
            onClick={() => app_up()}
            className="bg-green-500 border border-green-700 p-1 rounded text-white"
          >
            Start{" "}
          </button>
          <button
            onClick={() => app_stop()}
            className="bg-red-400 border border-red-700 p-1 rounded text-white"
          >
            Stop{" "}
          </button>
          <button
            onClick={() => app_down()}
            className="bg-red-400 border border-red-700 p-1 rounded text-white"
          >
            Down{" "}
          </button>
          <button
            onClick={() => app_logs()}
            className="bg-red-400 border border-red-700 p-1 rounded text-white"
          >
            Logs{" "}
          </button>
          <button
            onClick={() => app_pull()}
            className="bg-red-400 border border-red-700 p-1 rounded text-white"
          >
            Pull{" "}
          </button>
        </div>
      </div>
      <div className="flex flex-col h-full p-2  overflow-y-scroll">
        <div className="border-1 border-gray-300 rounded p-2 bg-white text-black">
          <div className="flex flex-row gap-2 justify-between">
            <div className="flex flex-col gap-2">
              <div className="flex flex-col gap-2">
                <div className="font-bold">Information</div>
                <div className="grid grid-cols-2 gap-1">
                  <div className="font-light">Path</div>
                  <div className="font-light">{app.app_path}</div>
                  <div className="font-light">1.0.0</div>
                  <div className="font-light">Running</div>
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
            <div className="flex flex-col gap-2">
              <div className="font-bold">
                {advertisedSignals.length > 0 && <GrBeacon />}
              </div>
            </div>
          </div>
        </div>
        <div className="font-light mt-2">Status of Deployment</div>
        <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-6 xl:grid-cols-6 gap-2 mt-2">
          {services.map((c) => (
            <div
              className={`shadow shadow-lg border border-1 white p-2 p-3  text-slate-800 bg-slate-100 rounded rounded-md `}
            >
              <div className="flex flex-row justify-between">
                <div className="font-bold text-">{c.name}</div>
                <div
                  className={`h-3 w-3 rounded-full border ${getServiceColor(
                    c
                  )} inline-block`}
                ></div>
              </div>
              <div className="flex flex-col gap-2">
                {c.containers.map((c, index) => {
                  return (
                    <div
                      className={`group border border-1 white p-2 p-3  text-black rounded rounded-md ${getContainerColor(
                        c
                      )}`}
                    >
                      <div className="flex flex-row justify-between">
                        <div>
                          <div className="font-bold">Instance {index + 1}</div>
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
            .map((c) => (
              <div
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
        <div className="font-light mt-2">Log on</div>
        <pre>
          {rollingLog.map((l) => (
            <>
              {l}
              <br />
            </>
          ))}
        </pre>
      </div>
    </div>
  );
};

export const DashboardScreen: React.FC<{}> = (props) => {
  const { id } = useParams<{ id: string }>();
  const { apps } = useStorage();

  let app = apps.find((app) => app.name === id);

  return app ? <Dashboard app={app} /> : <>Could not find this app</>;
};
