import { useState } from "react";
import * as api from "./api";
import type { ComposeAction } from "./api";
import { Button } from "./components/ui/button";
import { Input } from "./components/ui/input";
import { useAlerter } from "./alerter/alerter-context";
import { Popover, PopoverContent } from "./components/ui/popover";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "./components/ui/dialog";
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
export const useComposeAction = (props: ComposeButtonProps) => {
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
  }
) => {
  const { run, running } = useComposeAction(props);
  const [open, setOpen] = useState(false);
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
}) => {
  const { alert } = useAlerter();
  const [open, setOpen] = useState(false);
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

/**
 * The confirmation the dangerous *menu items* use.
 *
 * A `Popover` cannot do this job from inside a dropdown: choosing the item closes the
 * menu, which unmounts the trigger before the popover has a chance to open, and the item
 * looks like it did nothing. A dialog driven by state the menu sets survives the menu
 * closing, because it is a sibling of it rather than a child.
 */
export const ConfirmDialog = ({
  open,
  onOpenChange,
  title,
  description,
  confirmTitle = "Yes",
  onConfirm,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: string;
  /** The label on the button that goes through with it. */
  confirmTitle?: string;
  onConfirm: () => void;
}) => (
  <Dialog open={open} onOpenChange={onOpenChange}>
    <DialogContent className="sm:max-w-md">
      <DialogHeader>
        <DialogTitle>{title}</DialogTitle>
        <DialogDescription>{description}</DialogDescription>
      </DialogHeader>
      <DialogFooter>
        <Button variant="outline" onClick={() => onOpenChange(false)}>
          Cancel
        </Button>
        <Button
          variant="destructive"
          onClick={() => {
            onOpenChange(false);
            onConfirm();
          }}
        >
          {confirmTitle}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
);

/**
 * A confirmation that will not take a click alone: the exact name has to be typed.
 *
 * For the one action nothing recovers from. The friction is the point — a hub is deleted
 * about once in its life, and a menu item that is one careless click away from removing
 * a folder and its database is the wrong shape for that, however loud its wording.
 *
 * Unlike `ConfirmDialog` this does not close before running. Deleting a hub takes
 * `docker compose down` with it, which is seconds rather than milliseconds, and it can
 * fail in a way worth reading — Docker not running is the common one. So the dialog holds
 * its ground, shows that it is working, and shows the error in place if one comes back.
 */
export const ConfirmByNameDialog = ({
  open,
  onOpenChange,
  title,
  description,
  /** Typed back exactly, character for character, before the button comes alive. */
  expected,
  confirmTitle,
  runningTitle,
  onConfirm,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: React.ReactNode;
  expected: string;
  confirmTitle: string;
  runningTitle?: string;
  onConfirm: () => Promise<void>;
}) => {
  const [typed, setTyped] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | undefined>();

  const matches = typed === expected;

  const close = (next: boolean) => {
    // Never mid-flight: the folder is being removed, and a closed dialog would leave the
    // page looking like nothing happened while it finished.
    if (running) return;
    if (!next) {
      setTyped("");
      setError(undefined);
    }
    onOpenChange(next);
  };

  const go = async () => {
    if (!matches || running) return;
    setRunning(true);
    setError(undefined);
    try {
      await onConfirm();
      setTyped("");
      onOpenChange(false);
    } catch (e) {
      setError(typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription asChild>
            <div className="flex flex-col gap-2">{description}</div>
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-2">
          <label htmlFor="confirm-name" className="text-sm">
            Type <span className="font-mono font-medium">{expected}</span> to confirm.
          </label>
          <Input
            id="confirm-name"
            autoFocus
            autoComplete="off"
            spellCheck={false}
            value={typed}
            disabled={running}
            placeholder={expected}
            onChange={(event) => setTyped(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                void go();
              }
            }}
          />
        </div>

        {error && (
          <div className="text-sm text-destructive break-words">{error}</div>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => close(false)} disabled={running}>
            Cancel
          </Button>
          <Button
            variant="destructive"
            disabled={!matches || running}
            className={running ? "animate-pulse" : undefined}
            onClick={() => void go()}
          >
            {running && runningTitle ? runningTitle : confirmTitle}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
