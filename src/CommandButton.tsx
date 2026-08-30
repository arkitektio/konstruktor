import { useRef, useState } from "react";
import { Loader2 } from "lucide-react";
import * as api from "./api";
import type { ComposeAction } from "./api";
import { Button } from "./components/ui/button";
import { cn } from "./utils";
import {
  advance,
  EMPTY_PROGRESS,
  newProgressState,
  type ComposeProgress,
} from "./compose-progress";
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
  /** The compose project name, so progress reads `db` rather than `hub-db-1`. */
  project?: string;
};

/**
 * Runs one `docker compose` action through the core and reports what happened.
 *
 * The output streams: compose names each container as it starts, stops or pulls it,
 * and `progress` is that narration turned into a fraction and a phrase, for the button
 * to show while the command is still going.
 */
export const useComposeAction = (props: ComposeButtonProps) => {
  const { alert } = useAlerter();
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<ComposeProgress>(EMPTY_PROGRESS);
  const state = useRef(newProgressState());

  const run = async () => {
    setRunning(true);
    state.current = newProgressState();
    setProgress(EMPTY_PROGRESS);
    try {
      await api.composeCommandStreamed(props.path, props.action, (line) =>
        setProgress(advance(state.current, line, props.project))
      );
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

  return { run, running, progress };
};

/**
 * The button while its command runs: a fill creeping in from the left edge for how far
 * compose has got, a spinner, and the current step in small print beside it.
 *
 * Only the fill and the spinner live inside the button. The step text is outside, to the
 * left, so the button keeps its width and the footer does not jump as the words change.
 */
const ProgressButton = ({
  title,
  runningTitle,
  running,
  progress,
  onClick,
  variant,
}: {
  title: string;
  runningTitle?: string;
  running: boolean;
  progress: ComposeProgress;
  onClick?: () => void;
  variant?: "default" | "destructive";
}) => (
  <span className="inline-flex items-center gap-2 min-w-0">
    {running && progress.step && (
      <span
        className="hidden sm:inline text-xs text-muted-foreground font-mono truncate max-w-[26ch]"
        title={`${progress.done} of ${progress.total}`}
      >
        {progress.step}
      </span>
    )}
    <Button
      variant={variant}
      onClick={onClick}
      disabled={running}
      className={cn("relative overflow-hidden", running && "disabled:opacity-100")}
      aria-busy={running}
    >
      {running && (
        <span
          aria-hidden
          className={cn(
            "absolute inset-y-0 left-0 bg-primary/15 transition-[width] duration-300 ease-out",
            progress.fraction === undefined && "w-1/6 animate-pulse"
          )}
          style={
            progress.fraction === undefined
              ? undefined
              : { width: `${Math.max(6, Math.round(progress.fraction * 100))}%` }
          }
        />
      )}
      <span className="relative flex items-center gap-2">
        {running && <Loader2 className="size-3.5 animate-spin" />}
        {running && runningTitle ? runningTitle : title}
        {running && progress.total > 0 && (
          <span className="text-xs tabular-nums text-muted-foreground">
            {progress.done}/{progress.total}
          </span>
        )}
      </span>
    </Button>
  </span>
);

export const CommandButton = (props: ComposeButtonProps) => {
  const { run, running, progress } = useComposeAction(props);

  return (
    <ProgressButton
      title={props.title}
      runningTitle={props.runningTitle}
      running={running}
      progress={progress}
      onClick={() => void run()}
    />
  );
};

export const DangerousCommandButton = (
  props: ComposeButtonProps & {
    confirmTitle?: string;
    confirmDescription?: string;
  }
) => {
  const { run, running, progress } = useComposeAction(props);
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <span>
          <ProgressButton
            title={props.title}
            runningTitle={props.runningTitle}
            running={running}
            progress={progress}
            onClick={() => setOpen(true)}
          />
        </span>
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
