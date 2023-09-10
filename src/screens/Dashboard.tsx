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
import { Button } from "../components/ui/button";
import { Page } from "../layout/Page";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../components/ui/card";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "../components/ui/tooltip";
import {
  Menubar,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarShortcut,
  MenubarTrigger,
} from "../components/ui/menubar";
import { Logo } from "../layout/Logo";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from "../components/ui/dialog";
import { DialogTrigger } from "@radix-ui/react-dialog";
import { LogoMenu, SettingsMenu } from "../components/AppMenu";
import { DoubleArrowUpIcon } from "@radix-ui/react-icons";

export const Dashboard: React.FC<{ app: App }> = ({ app }) => {
  const { call, status } = useCommunication();
  const { deleteApp } = useStorage();
  const navigate = useNavigate();
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
    <Page
      buttons={
        <>
          <Button asChild>
            <Link to="/">{"<"} Home</Link>
          </Button>
          <Button asChild>
            <Link to={`/logs/${app.name}`}>Logs</Link>
          </Button>
          <Button onClick={() => openFolder()}>Open Folder</Button>
          <CommandButton
            params={{
              program: "docker",
              args: ["compose", "up", "-d"],
              options: {
                cwd: app.path,
              },
            }}
            title="Start"
            runningTitle="Starting..."
          />
          <DangerousCommandButton
            params={{
              program: "docker",
              args: ["compose", "stop"],
              options: {
                cwd: app.path,
              },
            }}
            title="Stop"
            runningTitle="Stopping..."
            confirmTitle="Are you sure you want to stop?"
            confirmDescription="This will stop all containers, everyone will be disconnected, while data should be kept, this might 
            cause unexpected results, if apps are still running."
          />
        </>
      }
      menu={
        <Menubar className="flex-initial border-0 justify-between">
          <LogoMenu />
          <div className="flex flex-row">
            <MenubarMenu>
              <MenubarTrigger>Danger</MenubarTrigger>
              <MenubarContent>
                <Dialog>
                  <DialogTrigger asChild>
                    <MenubarItem onSelect={(e) => e.preventDefault()}>
                      Update
                      <MenubarShortcut>⌘D</MenubarShortcut>
                    </MenubarItem>
                  </DialogTrigger>
                  <DialogContent>
                    <DialogTitle>Update</DialogTitle>
                    <DialogDescription>
                      Updating is a slighly dangerous operation. It will pull
                      the latest images from the registry which will replace
                      your current images on the next start.
                    </DialogDescription>
                    <DialogFooter>
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
                        confirmTitle="Are you sure you want to update?"
                        confirmDescription="Updating will not interrupt your services, but will replace the images with the latest ones from the registry, on the next start"
                      />
                    </DialogFooter>
                  </DialogContent>
                </Dialog>

                <Dialog>
                  <DialogTrigger asChild>
                    <MenubarItem onSelect={(e) => e.preventDefault()}>
                      Delete
                      <MenubarShortcut>⌘D</MenubarShortcut>
                    </MenubarItem>
                  </DialogTrigger>
                  <DialogContent>
                    <DialogTitle>Delete</DialogTitle>
                    <DialogDescription>
                      When deleting this app all of your data will be lost!
                    </DialogDescription>
                    <DialogFooter>
                      <DangerousCommandButton
                        params={{
                          program: "docker",
                          args: ["compose", "down"],
                          options: {
                            cwd: app.path,
                          },
                        }}
                        callback={() =>
                          deleteApp(app).then(() => navigate("/"))
                        }
                        title="Tear down and Delete"
                        confirmTitle="Are you really sure you want to delete this app?"
                        confirmDescription="This is an irreversible action"
                      />
                    </DialogFooter>
                  </DialogContent>
                </Dialog>
              </MenubarContent>
            </MenubarMenu>
            <SettingsMenu />
          </div>
        </Menubar>
      }
    >
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
        <>
          <CardHeader>
            <CardTitle className="cursor-pointer" onClick={() => openFolder()}>
              {app.name}
            </CardTitle>
            <CardDescription>{"build with Channel"}</CardDescription>
          </CardHeader>

          <CardContent>
            <div className="text-md mt-2">Status of Deployment</div>
            <div className="text-sm text-muted-foreground">
              These are the active services
            </div>
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
            <div className="text-md mt-5">Advertise</div>
            <div className="text-sm text-muted-foreground">
              Activating items here will make this deployment visble to apps in
              the respecting network.
            </div>
            <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-4 xl:grid-cols-4 gap-2 mt-3">
              {bindings
                .filter((c) => c.broadcast)
                .map((c, index) => (
                  <Tooltip>
                    <TooltipTrigger>
                      <Card
                        key={index}
                        className={`group cursor-pointer ${
                          is_advertised(c) ? "opacity-100" : "opacity-50"
                        } ${is_advertised(c) && advertise ? "" : ""}`}
                        onClick={() => toggleAdvertised(c)}
                      >
                        <CardHeader className="flex flex-row justify-between p-2 truncate elipsis">
                          <CardTitle className="text-md">{c.host}</CardTitle>
                        </CardHeader>
                        <CardContent className="p-2 truncate">
                          {is_advertised(c) ? (
                            <div className="flex flex-row text-xs">
                              <div className="my-auto">
                                <DoubleArrowUpIcon />
                              </div>
                              <div className="my-auto ml-1">{c.broadcast}</div>
                            </div>
                          ) : (
                            <div className="flex flex-row text-xs text-xs group-hover:visible invisible">
                              <div className="my-auto">
                                <DoubleArrowUpIcon />
                              </div>
                              <div className="my-auto ml-1">{c.broadcast}</div>
                            </div>
                          )}
                        </CardContent>
                      </Card>
                    </TooltipTrigger>
                    <TooltipContent>
                      <div className="text-xs flex flex-col">
                        {c.host}
                        <div className="font-bold">
                          {is_advertised(c)
                            ? "This is advertising"
                            : "This is not advertised"}
                        </div>
                      </div>
                    </TooltipContent>
                  </Tooltip>
                ))}
            </div>
          </CardContent>
        </>
      )}
    </Page>
  );
};

export const DashboardScreen: React.FC<{}> = (props) => {
  const { id } = useParams<{ id: string }>();
  const { apps } = useStorage();

  let app = apps.find((app) => app.name === id);

  return app ? <Dashboard app={app} /> : <>Could not find this app</>;
};
