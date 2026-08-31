import { CircleCheck, CircleSlash, Loader2, TriangleAlert } from "lucide-react";
import { open as openExternal } from "@tauri-apps/plugin-shell";

import type { CreateEvent } from "../../api";
import { Alert } from "../../components/ui/alert";
import { Button } from "../../components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "../../components/ui/dialog";
import { ScrollArea } from "../../components/ui/scroll-area";

/**
 * Creating a hub, as it happens.
 *
 * This is where the device code appears. Authorization used to be a wizard step, which
 * meant its result could go stale against answers changed afterwards; it happens inside
 * the one `create_hub` call now, so the code shows up here instead — and the whole
 * "authorize again" mechanism is gone.
 *
 * The body is {@link InstallPanel} rather than a dialog of its own, because the wizards
 * show it as their last step: waiting for somebody in another building to accept a code
 * is not a modal moment, and a dialog over a wizard hid the rail that says where in the
 * flow this is. The dialog wrapper stays for the connect screen, which is a `Page`.
 */
export type StagedEvent = Extract<CreateEvent, { event: "staged" }>;
export type WaitingEvent = Extract<CreateEvent, { event: "waiting" }>;

export type CreateState = {
  running: boolean;
  /** The last event, which is what decides the heading. */
  event?: CreateEvent;
  /**
   * The device code, kept apart from `event`. It has to stay on screen for as long as
   * somebody could still act on it, and the poll loop starts sending `waiting` within
   * milliseconds of it — so reading it off the *last* event would show it to nobody.
   */
  staged?: StagedEvent;
  /** The most recent poll, for the countdown next to the code. */
  waiting?: WaitingEvent;
  logs: string[];
  error: string | null;
  done: boolean;
  /**
   * The user asked to stop. Set the moment Cancel is pressed and kept once the call has
   * returned, because the error it returns is "Cancelled." — an outcome somebody chose,
   * which must not be shown as a failure.
   */
  cancelled: boolean;
};

export const emptyCreateState: CreateState = {
  running: false,
  logs: [],
  error: null,
  done: false,
  cancelled: false,
};

/**
 * Fold one event into the state. Shared, because the wizard and the re-authorize screen
 * run the same flow and used to keep their own near-copies of this.
 */
export const reduceCreate = (previous: CreateState, event: CreateEvent): CreateState => ({
  ...previous,
  event,
  staged:
    event.event === "staged"
      ? event
      : // Accepted — nothing left for anybody to type in.
        event.event === "granted"
        ? undefined
        : previous.staged,
  waiting: event.event === "waiting" ? event : previous.waiting,
  logs:
    event.event === "log"
      ? [...previous.logs, event.line]
      : event.event === "writing"
        ? [...previous.logs, `wrote ${event.file}`]
        : event.event === "cloning"
          ? [
              ...previous.logs,
              `cloning ${event.repo}${event.branch ? ` at ${event.branch}` : ""} into mounts/${event.service}`,
            ]
          : previous.logs,
});

export const heading = (state: CreateState, kind: string): string => {
  if (state.cancelled && !state.done) return "Stopped";
  if (state.error) return "That did not work";
  if (state.done) return `Your ${kind} is written`;

  switch (state.event?.event) {
    case "checking-docker":
      return "Checking Docker…";
    case "building":
      return "Building the profile…";
    case "staged":
    case "waiting":
      return "Waiting to be accepted";
    case "granted":
      return "Accepted";
    case "writing":
      return "Writing the deployment…";
    case "cloning":
      return "Checking the source out…";
    case "starting":
    case "log":
      return "Starting the stack…";
    default:
      return `Creating your ${kind}`;
  }
};

/** The line under the heading — what just happened, or what is being waited for. */
export const explanation = (state: CreateState, kind: string): string => {
  if (state.cancelled && !state.done)
    return `Nothing was written. The ${kind} was never accepted, so the code you saw is now worthless — go back and create it again when you are ready.`;
  if (state.error) return "Nothing was written unless the step below says otherwise.";
  if (state.done)
    return "The deployment is in your folder and registered here. Nothing is running yet — start it from the dashboard.";
  return "Konstruktor is building, authorizing and writing your deployment.";
};

/** The icon beside the heading, in both the dialog and the wizard step. */
export const StateIcon = ({
  state,
  className = "size-4",
}: {
  state: CreateState;
  className?: string;
}) =>
  state.cancelled && !state.done ? (
    <CircleSlash className={`${className} text-muted-foreground`} />
  ) : state.error ? (
    <TriangleAlert className={`${className} text-destructive`} />
  ) : state.done ? (
    <CircleCheck className={`${className} text-primary`} />
  ) : (
    <Loader2 className={`${className} animate-spin text-muted-foreground`} />
  );

/**
 * Everything the progress view shows: the error, the device code somebody has to accept,
 * and the log. No heading — the dialog and the wizard step each wear their own.
 */
export const InstallPanel = ({
  state,
  kind = "hub",
  onCancel,
}: {
  state: CreateState;
  kind?: string;
  /**
   * Stop waiting. Offered next to the code because that is where the waiting is visible;
   * the wizard passes nothing and puts it in the footer with its other buttons.
   */
  onCancel?: () => void;
}) => {
  const { staged, waiting } = state;
  const stopped = state.cancelled && !state.done;

  return (
    <div className="flex flex-col gap-4">
      {/* A cancelled run is not a failure, so its "Cancelled." is not shown in red. */}
      {state.error && !stopped && <Alert variant="destructive">{state.error}</Alert>}

      {/* The one part a person has to act on. */}
      {staged && !state.done && !state.error && (
        <div className="rounded-lg border border-primary/60 bg-primary/5 p-4 flex flex-col gap-3">
          <div className="text-sm">
            Somebody with an account on your coordination server has to accept this{" "}
            {kind}.
          </div>
          <div className="flex items-center gap-3">
            <code className="text-lg font-semibold tracking-widest">
              {staged.user_code}
            </code>
            <Button
              size="sm"
              onClick={() => openExternal(staged.verification_uri_complete)}
            >
              Open the page
            </Button>
            {onCancel && (
              <Button
                size="sm"
                variant="ghost"
                disabled={state.cancelled}
                onClick={onCancel}
              >
                {state.cancelled ? "Stopping…" : "Stop waiting"}
              </Button>
            )}
          </div>
          <code className="text-xs text-muted-foreground break-all">
            {staged.verification_uri_complete}
          </code>
          {waiting && (
            <div className="text-xs text-muted-foreground">
              Waiting… {Math.floor(waiting.seconds_left / 60)}m
              {String(waiting.seconds_left % 60).padStart(2, "0")}s left
            </div>
          )}
        </div>
      )}

      {state.logs.length > 0 && (
        <ScrollArea className="w-full h-[35vh] bg-background border border-border p-3 rounded-md">
          <div className="flex flex-col gap-0.5">
            {state.logs.map((line, index) => (
              <code
                key={index}
                className="text-xs whitespace-pre-wrap break-all text-muted-foreground"
              >
                {line}
              </code>
            ))}
          </div>
        </ScrollArea>
      )}
    </div>
  );
};

export const InstallProgress = ({
  open,
  state,
  onClose,
  onCancel,
  /** What is being created, for the headings. Both wizards share this dialog. */
  kind = "hub",
}: {
  open: boolean;
  state: CreateState;
  onClose: () => void;
  onCancel?: () => void;
  kind?: string;
}) => {
  const finished = state.done || state.error !== null;

  return (
    <Dialog open={open} onOpenChange={(next) => !next && finished && onClose()}>
      <DialogContent className="bg-card max-w-3xl">
        <DialogTitle className="flex items-center gap-2">
          <StateIcon state={state} />
          {heading(state, kind)}
        </DialogTitle>

        <DialogDescription>{explanation(state, kind)}</DialogDescription>

        <InstallPanel state={state} kind={kind} onCancel={onCancel} />
      </DialogContent>
    </Dialog>
  );
};
