import { invoke } from "@tauri-apps/api";
import React, { useEffect, useState } from "react";
import { TbReload } from "react-icons/tb";
import { Link, useNavigate, useParams } from "react-router-dom";
import { useBeacon } from "../beacon/context";
import { useCommunication } from "../communication/communication-context";
import { ResponsiveGrid } from "../layout/ResponsiveGrid";
import { App, useStorage } from "../storage/storage-context";
import { BeaconInterface } from "../types";

import { open } from "@tauri-apps/api/shell";

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

  return "bg-red-500 border-red-600";
};

const getContainerColor = (container: Container) => {
  if (container.state == "running") {
    return "border-green-600";
  }
  if (container.state == "exited") {
    return "border-red-600";
  }
};


import { DialogTrigger } from "@radix-ui/react-dialog";
import { DoubleArrowUpIcon } from "@radix-ui/react-icons";
import { BaseDirectory, FileEntry, readDir, readTextFile, writeTextFile } from "@tauri-apps/api/fs";
import { CommandButton, DangerousButton, DangerousCommandButton } from "../CommandButton";
import { LogoMenu, SettingsMenu } from "../components/AppMenu";
import { Button } from "../components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from "../components/ui/dialog";
import {
  Menubar,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarShortcut,
  MenubarTrigger,
} from "../components/ui/menubar";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "../components/ui/tooltip";
import { useCommand, useLazyCommand } from "../hooks/useCommand";
import { Page } from "../layout/Page";
import { useSettings } from "../settings/settings-context";
import { parse, stringify } from "yaml";
import { Alert } from "../components/ui/alert";
import * as yup from "yup";
import { ScrollArea } from "../components/ui/scroll-area";
import { f } from "@tauri-apps/api/path-c062430b";


export const KonstruktSchema = yup.object().shape({
  pulled: yup.date().required("Required"),
});

export type KonstruktInfo = yup.InferType<typeof KonstruktSchema>;


export const checkHealth = async (
  healthz: string,
): Promise<boolean> => {
  try {
    //console.log(`Checking ${name} on ${host}:${port}`)
    const res = await fetch(`${healthz}/?format=json`, {
      headers: {
        'Content-Type': 'application/json',
      },
      method: 'GET',
    })

    if (res.ok)
      return true;
    else {
      return false;
    }
  } catch (e) {
    return false;
  }
}

export const HealthIndicator = (props: { healthz: string | undefined, s: any }) => {

  const [healthy, setHealthy] = useState<boolean>(props.healthz == undefined ? true : false);


  useEffect(() => {
    if (!props.healthz) {
      return;
    }

    let interval = setInterval(() => {
      if (!props.healthz) {
        return;
      }
      checkHealth(props.healthz).then((res) => setHealthy(res));
    }

    , 3000);

    return () => clearInterval(interval);
  }, [props.healthz]);

  

  return props.healthz ? <div
  className={`h-3 w-3 rounded-full border ${healthy ? "bg-green-400 border-green-600" : "border-red-600"} inline-block`}
></div> : <div
  className={`h-3 w-3 rounded-full border ${getServiceColor(
    props.s
  )} inline-block`}
></div>

}



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
  const [konstruktInfo, setKonstruktInfo] = useState<KonstruktInfo | null>(null);
  const [countDown, setCountDown] = useState<number>(4);
  const [dialogOpen, setDialogOpen] = useState<boolean>(false);
  const [dialogStatus, setDialogStatus] = useState<string>("");

  const openFolder = async () => {
    await open(app.path);
    console.log("opened");
  };

  const { run, logs, finished } = useLazyCommand({
    logLength: 50,
  });

  const writeNewInfo = async (konstruktInfo: KonstruktInfo) => {
    const written = await writeTextFile(
      `apps/${app.name}/konstruktor.yaml`,
      stringify(konstruktInfo),
      {
        dir: BaseDirectory.App,
      }
    );
    console.log(written);
    setRetrigger(!retrigger);
  };
    


  const pull = async () => {
      setDialogOpen(true);
      setDialogStatus("Pulling Images...");
      await run({
        program: "docker",
        args: ["compose", "pull"],
        options: {
          cwd: app.path,
      }})

      await writeNewInfo({...konstruktInfo, pulled: new Date()});
      setDialogStatus("Finished pulling images");
      setDialogOpen(false);



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

  

  const { run: down } = useCommand({
    program: "docker",
    args: ["compose", "down"],
    options: {
      cwd: app.path,
    },
  });

  let deployment = app.name.toLowerCase();
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


  const processKonstruktorYAML = async (entry: FileEntry) => {
    
    try {
      let setup_string = await readTextFile(entry.path, {
        dir: BaseDirectory.App,
      });

      let setup = parse(setup_string);
      
      try {
      const validatedInfo = await KonstruktSchema.validate(setup);
      setKonstruktInfo(validatedInfo);
      }
      catch (e) {
        console.error(e);
      }
    } catch (e) {
      console.error(e);
    }
  }



  const processEntries = (entries: FileEntry[]) => {
    for (const entry of entries) {
      if (entry.name == "docker-compose.yaml") {
        setInitialized(true);
      }
      if (entry.name == "konstruktor.yaml") {
        processKonstruktorYAML(entry);
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

  const lok_port = 11000;





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
        <div className="flex flex-justify-between flex-row gap-2 w-full">
          <div className="flex initial flex-row gap-2">
          <Button asChild>
            <Link to="/">Home</Link>
          </Button>
          <Button asChild>
            <Link to={`/logs/${app.name}`}>Logs</Link>
          </Button>
          </div>
          <div className="flex-grow"></div>
          <div className="flex flex-row gap-2">
          {konstruktInfo?.pulled ? <CommandButton
            params={{
              program: "docker",
              args: ["compose", "up", "-d"],
              options: {
                cwd: app.path,
              },
            }}
            title="Start"
            runningTitle="Starting..."
          /> :
          <Button
            onClick={() => {
                pull();
              }
            }
            className="w-20"
          >
            Pull
          </Button>


           }
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
          </div>
        </div>
      }
      menu={
        <Menubar className="flex-initial border-0 justify-between">
          <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
              <DialogContent className="bg-card">
                <DialogTitle>{dialogStatus}</DialogTitle>
                {logs.length > 0 && (
                  <ScrollArea className="w-full h-[50vh] bg-gray-900 p-3 rounded rounded-md overflow-scroll">
                    {logs.map((p, i) => (
                      <div className="w-full grid grid-cols-12 ">
                        <div className="col-span-1">{i}</div>
                        <div className="col-span-11"> {p}</div>
                      </div>
                    ))}
                  </ScrollArea>
                )}
              </DialogContent>
            </Dialog>
          <LogoMenu />
          <div className="flex flex-row">
            <MenubarMenu>
              <MenubarTrigger>Danger</MenubarTrigger>
              <MenubarContent>
                <Dialog>
                  <DialogTrigger asChild>
                    <MenubarItem onSelect={(e) => e.preventDefault()}>
                      Update
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
                          deleteApp(app.name).then(() => navigate("/")).catch(alert)
                        }
                        title="Tear down and Delete"
                        confirmTitle="Are you really sure you want to delete this app?"
                        confirmDescription="This is an irreversible action"
                      />
                    </DialogFooter>
                  </DialogContent>
                </Dialog>
                <Dialog>
                  <DialogTrigger asChild>
                    <MenubarItem onSelect={(e) => e.preventDefault()}>
                      Purge
                    </MenubarItem>
                  </DialogTrigger>
                  <DialogContent>
                    <DialogTitle>Purge</DialogTitle>
                    <DialogDescription>
                      When purging this app all of your data will be lost!
                      (This circumvents running the docker-compose down command,)
                    </DialogDescription>
                    <DialogFooter>
                      <DangerousButton
                        callback={() =>
                          deleteApp(app.name).then(() => navigate("/")).catch(alert)
                        }
                        title="Purge down and Delete"
                        confirmTitle="Are you really sure you want to purge this app?"
                        confirmDescription="This will irreversibly delete all of your data in the directory and remove the app from the list."
                      />
                    </DialogFooter>
                  </DialogContent>
                </Dialog>
                <Dialog>
                  <DialogTrigger asChild>
                    <MenubarItem onSelect={(e) => e.preventDefault()}>
                      Reset
                    </MenubarItem>
                  </DialogTrigger>
                  <DialogContent>
                    <DialogTitle>Reset</DialogTitle>
                    <DialogDescription>
                      Resetting will delete all of your data, and then rebuild
                      the app, in a way that is similar to a fresh install.
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
                        title="Reset"
                        confirmTitle="Are you really sure you want to reset this app?"
                        confirmDescription="This is an irreversible action"
                      />
                    </DialogFooter>
                  </DialogContent>
                </Dialog>
                <MenubarItem onSelect={(e) => setRetrigger(!retrigger)}>
                      Recheck App
                </MenubarItem>
              </MenubarContent>
            </MenubarMenu>
            <SettingsMenu />
          </div>
        </Menubar>
      }
    >
      {!konstruktInfo ? (
        <div className="flex-grow flex flex-col h-full p-2 overflow-y-scroll">
          <div className="font-light text-5xl mt-2">{app.name} </div>
          <div className="font-light text-3xl mt-2">is not yet initialized </div>
          <div className="mt-5 max-w-xl">
            We didn't find a konstruktor.yaml file for this app. This could be because you have not yet initialized it.
            Just click the button below to initialize it.
          </div>
          <div className="mt-5 flex flex-col text-1xl max-w-xl gap-2 mt-3 font-bold">
            Lets initialize it!
          </div>
          <Button
            onClick={() => {
                pull();
              }
            }
            className="w-40 p-7 text-2xl mt-2"
          >
            Initialize
          </Button>
        </div>
      ) : (
        <>
          <CardHeader className="cursor-pointer"  onClick={() => openFolder()}>
            <CardTitle >
              {app.name}
            </CardTitle>
            <CardDescription> located in {app.path}</CardDescription>
          </CardHeader>

          <CardContent>
            <div className="text-md mt-2">Status of Deployment</div>
            <div className="text-sm text-muted-foreground">
              These are the active services
            </div>
            <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-6 xl:grid-cols-6 gap-2 mt-2">
              {services.map((s, index) => (
                <Card
                  key={index}
                  className={`group`}
                >
                  <CardHeader className="flex flex-row justify-between  p-3">
                    <CardTitle>
                    <Link
                      to={`/logs/${app.name}/service/${s.name}`}
                      className=""
                    >
                      {s.name}
                    </Link>
                    
                    </CardTitle>
                    

                    <HealthIndicator healthz={s.containers?.at(0)?.labels["arkitekt.healthz"]} s={s}/>
                  </CardHeader>
                  <CardContent className="flex flex-col gap-2 p-3">
                  {s.containers?.at(0)?.labels["arkitekt.description"] && <div className="text-xs">{s.containers?.at(0)?.labels["arkitekt.description"]}</div>}
                  


                    {s.containers.map((c, index) => {
                      return (
                        <div
                          className={`group border border-1 white p-2 p-3 rounded rounded-md ${getContainerColor(
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
                    <div className="flex flex-row gap-1">
                    {s.containers?.at(0)?.labels["arkitekt.link"] && <Button onClick={() => open(s.containers?.at(0)?.labels["arkitekt.link"] || "")} className="text-xs flex-1">Open Service</Button>}
                    {s.containers?.at(0)?.labels["arkitekt.healthz"] && <Button onClick={() => open(s.containers?.at(0)?.labels["arkitekt.healthz"] || "")} className="text-xs flex-1">Open Healthz</Button>}
                    </div>

                  </CardContent>
                </Card>
              ))}
              {services.length == 0 && (
                <Alert className="col-span-6 mt-3">
                  <div className="text-md">No services found</div>
                  <div className="text-sm text-muted-foreground">
                    It appears that you have never started this app before. Press start
                    to start it.
                    </div>
                    </Alert>
                    )}
            </div>
            {services.length != 0 && <><div className="text-md mt-5">Advertise</div>
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
            </>}
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
