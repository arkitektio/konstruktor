import {
  Child,
  ChildProcess,
  Command,
  SpawnOptions,
} from "@tauri-apps/api/shell";
import { useEffect, useState } from "react";

export type CommandParams = {
  program: string;
  args?: string | string[];
  options?: SpawnOptions;
  running?: boolean;
  onClose?: (code: number) => void;
  onError?: (error: string) => void;
  onSpawned?: (child: Child) => void;
  logLength?: number;
};

export const useCommand = (params: CommandParams) => {
  const [command] = useState(
    () => new Command(params.program, params.args, params.options)
  );
  const [logs, setLogs] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState<boolean>(false);
  const [finished, setFinished] = useState<ChildProcess | null>(null);

  const listener = (data: string) => {
    console.log(data);
    setLogs((prevLogs) => [data, ...prevLogs].slice(0, params.logLength));
  };

  const errorListener = (data: string) => {
    console.error(data);
    setLogs((prevLogs) => [data, ...prevLogs].slice(0, params.logLength));
    setError(data);
  };

  const closeListener = (code: number) => {
    console.log("close", code);
    if (code === 0) {
      params.onClose && params.onClose(code);
    } else {
      params.onError && params.onError(error ? error : "Error");
    }
  };

  useEffect(() => {
    command.on("close", closeListener);

    command.stderr.on("data", errorListener);
    command.stdout.on("data", listener);

    return () => {
      command.off("close", closeListener);
      command.stderr.off("data", errorListener);
      command.stdout.off("data", listener);
    };
  }, [command]);

  const run = () => {
    console.log("running");
    setFinished(null);
    setRunning(true);
    return command.execute().then((x) => {
      setFinished(x);
      setRunning(false);
      return x;
    });
  };

  return {
    run,
    command,
    logs,
    error,
    running,
    finished,
  };
};

export type LazyCommandParams = {
  onClose?: (code: number) => void;
  onError?: (error: string) => void;
  onSpawned?: (child: Child) => void;
  logLength?: number;
};

export type RunParams = {
  program: string;
  args?: string | string[];
  options?: SpawnOptions;
};

export const useLazyCommand = (params: LazyCommandParams) => {
  const [command, setCommand] = useState<Command | null>(null);

  const [logs, setLogs] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [finished, setFinished] = useState<ChildProcess | null>(null);

  const listener = (data: string) => {
    setLogs((prevLogs) => [...prevLogs, data].slice(-(params.logLength || 50)));
  };

  const errorListener = (data: string) => {
    setError(data);
  };

  const closeListener = (code: number) => {
    if (code === 0) {
      params.onClose && params.onClose(code);
    } else {
      params.onError && params.onError(error ? error : "Error");
    }
  };

  useEffect(() => {
    if (command) {
      command.on("close", closeListener);

      command.stderr.on("data", errorListener);
      command.stdout.on("data", listener);

      return () => {
        command.off("close", closeListener);
        command.stderr.off("data", errorListener);
        command.stdout.off("data", listener);
      };
    }
  }, [command]);

  const run = async (params: RunParams) => {
    let command = new Command(params.program, params.args, params.options);
    setFinished(null);
    setCommand(command);
    let x = await command.execute();
    setCommand(command);
    setFinished(x);
    return x;
  };

  return {
    run,
    command,
    logs,
    error,
    finished,
  };
};
