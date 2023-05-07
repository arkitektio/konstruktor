import { Child, Command, SpawnOptions } from "@tauri-apps/api/shell";
import { useEffect, useState } from "react";

export type CommandParams = {
    program: string, 
    args?: string | string[],
    options?: SpawnOptions,
    onClose?: (code: number) => void,
    onError?: (error: string) => void,
    onSpawned?: (child: Child) => void,
}

const useCommand = (params: CommandParams) => {
    const [command] = useState(() => new Command(params.program, params.args, params.options));
    const [logs, setLogs] = useState<string[]>([]);
    const [error, setError] = useState<string | null>(null);

    const listener = (data: string) => {
        setLogs(prevLogs => [...prevLogs, data]);
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
        
        command.on("close", closeListener);

        command.stderr.on("data", errorListener);
        command.stdout.on("data", listener);

        return () => {
            command.off("close", closeListener);
            command.stderr.off("data", errorListener);
            command.stdout.off("data", listener);
        };
    }, [command]);


    const run = async() => {
        return await command.execute();
    };

    const spawn = async() => {
        return await command.spawn();
    };


    return {
        spawn,
        run,
        command,
        logs, 
        error
    };
};