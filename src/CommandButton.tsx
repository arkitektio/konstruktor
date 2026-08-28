import { useState } from "react";
import * as api from "./api";
import type { ComposeAction } from "./api";
import { Button } from "./components/ui/button";
import { useAlerter } from "./alerter/alerter-context";
import { Popover, PopoverContent } from "./components/ui/popover";
import { PopoverClose, PopoverTrigger } from "@/components/ui/popover";

export type ComposeButtonProps = {
  /** The deployment folder to run in. */
  path: string;
  action: ComposeAction;
  title: string;
  callback?: () => void;
  runningTitle?: string;
};

/**
 * Runs one `docker compose` action through the core and reports what happened.
 *
 * The output is buffered rather than streamed: every one of these runs to completion,
 * and nothing here displays output while a command is still going.
 */
const useComposeAction = (props: ComposeButtonProps) => {
  const { alert } = useAlerter();
  const [running, setRunning] = useState(false);

  const run = async () => {
    setRunning(true);
    try {
      await api.composeCommand(props.path, props.action);
      props.callback?.();
    } catch (error) {
      alert({
        error: `Error while running ${props.title}`,
        message: typeof error === "string" ? error : String(error),
        subtitle: "docker compose refused the command.",
      });
    } finally {
      setRunning(false);
    }
  };

  return { run, running };
};

export const CommandButton = (props: ComposeButtonProps) => {
  const { run, running } = useComposeAction(props);

  return (
    <>
      <Button
        onClick={run}
        disabled={running}
        className={running ? "animate-pulse" : undefined}
      >
        {running && props.runningTitle ? props.runningTitle : props.title}
      </Button>
    </>
  );
};

export const DangerousCommandButton = (
  props: ComposeButtonProps & {
    confirmTitle?: string;
    confirmDescription?: string;
    to?: number;
  }
) => {
  const { run, running } = useComposeAction(props);
  const [open, setOpen] = useState(false);
  const to = props.to || 10;
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button onClick={() => setOpen(true)}>
          {running && props.runningTitle ? props.runningTitle : props.title}
        </Button>
      </PopoverTrigger>
      <PopoverContent>
        <div className="flex flex-col gap-1">
          <div className="text-md">{props.confirmTitle || "Are you sure?"}</div>
          <div className="text-xs text-muted-foreground">
            {props.confirmDescription || "This might cause unexpected results"}
          </div>
          <div className="flex flex-row gap-2 w-full mt-2">
            <Button
              className="w-full"
              onClick={() => {
                setOpen(false);
                void run();
              }}
            >
              Yes
            </Button>

            <PopoverClose asChild>
              <Button className="w-full">No</Button>
            </PopoverClose>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
};


export const DangerousButton = (props: {
  title: string;
  callback: () => void;
  confirmTitle?: string;
  confirmDescription?: string;
  runningTitle?: string;
  to?: number;
}) => {
  const { alert } = useAlerter();
  const [open, setOpen] = useState(false);
  const to = props.to || 10;
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button onClick={() => setOpen(true)}>
          {props.runningTitle ? props.runningTitle : props.title}
        </Button>
      </PopoverTrigger>
      <PopoverContent>
        <div className="flex flex-col gap-1">
          <div className="text-md">{props.confirmTitle || "Are you sure?"}</div>
          <div className="text-xs text-muted-foreground">
            {props.confirmDescription || "This might cause unexpected results"}
          </div>
          <div className="flex flex-row gap-2 w-full mt-2">
            <Button
              className="w-full"
              onClick={() => {
                console.log("run");
                setOpen(false);
                props.callback();
              }}
            >
              Yes
            </Button>

            <PopoverClose asChild>
              <Button className="w-full">No</Button>
            </PopoverClose>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
};
