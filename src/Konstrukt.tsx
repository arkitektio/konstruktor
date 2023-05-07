import { ChildProcess } from "@tauri-apps/api/shell";
import { CommandParams, useCommand } from "./hooks/useCommand";
import { callbackify } from "util";
import { useSettings } from "./settings/settings-context";
import { App } from "./storage/storage-context";

export const Konstrukt = ({ app, callback }: { app: App; callback: any }) => {
  const { settings } = useSettings();
  const {
    run: konstrukt,
    logs: konstruktlogs,
    running: konstruktrun,
    finished: konstruktfinished,
  } = useCommand({
    program: "docker",
    args: ["run", "--rm", "-v", `${app.path}:/app/init`, settings.baker],
    options: {
      cwd: app.path,
    },
    logLength: 20,
  });

  const {
    run: pull,
    logs: pulllogs,
    running: pullrun,
    finished: pullfinished,
  } = useCommand({
    program: "docker",
    args: ["compose", "pull"],
    options: {
      cwd: app.path,
    },
    logLength: 20,
  });

  const bakeAndPull = async () => {
    console.log("Konstruktion");
    await konstrukt();
    console.log("Pull");
    await pull();
    console.log("Pulling done");
    callback();
  };

  const running = konstruktrun || pullrun;

  return (
    <>
      <button
        onClick={() => {
          bakeAndPull();
        }}
        disabled={pullrun || konstruktrun}
        className={`border cursor-pointer shadow-md shadow hover:bg-gray-200 border-1 border-gray-300 rounded p-2 bg-white text-black ${
          running ? "animate-pulse" : ""
        }`}
      >
        {konstruktrun && "Building..."}
        {pullrun && "Pulling..."}
        {!running && "Lets go!"}
      </button>
      {konstruktlogs && (
        <>
          <div className="font-light">Designing</div>
          <div
            className={`${
              konstruktfinished
                ? konstruktfinished.code == 1
                  ? "bg-red-600"
                  : "bg-green-600"
                : "bg-gray-800"
            } text-white p-3 rounded rounded-md overflow-y-scroll max-w-3xl`}
          >
            <pre>
              <>
                {konstruktlogs.map((log, index) => (
                  <>
                    {log}
                    <br />
                  </>
                ))}
              </>
            </pre>
          </div>
        </>
      )}
      {pulllogs && (
        <>
          <div className="font-light">Getting Material</div>
          <div
            className={`${
              pullfinished
                ? pullfinished.code == 1
                  ? "bg-red-600"
                  : "bg-green-600"
                : "bg-gray-800"
            } text-white p-3 rounded rounded-md overflow-y-scroll max-w-3xl`}
          >
            <pre>
              <>
                {pulllogs.map((log, index) => (
                  <>
                    {log}
                    <br />
                  </>
                ))}
              </>
            </pre>
          </div>
        </>
      )}
    </>
  );
};
