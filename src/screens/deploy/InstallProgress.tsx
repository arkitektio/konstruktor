import { CircleCheck, Loader2, TriangleAlert } from "lucide-react";
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
};

export const emptyCreateState: CreateState = {
  running: false,
  logs: [],
  error: null,
  done: false,
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

const heading = (state: CreateState, kind: string): string => {
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

export const InstallProgress = ({
  open,
  state,
  onClose,
  /** What is being created, for the headings. Both wizards share this dialog. */
  kind = "hub",
}: {
  open: boolean;
  state: CreateState;
  onClose: () => void;
  kind?: string;
}) => {
  const finished = state.done || state.error !== null;
  const { staged, waiting } = state;

  return (
    <Dialog open={open} onOpenChange={(next) => !next && finished && onClose()}>
      <DialogContent className="bg-card max-w-3xl">
        <DialogTitle className="flex items-center gap-2">
          {state.error ? (
            <TriangleAlert className="size-4 text-destructive" />
          ) : state.done ? (
            <CircleCheck className="size-4 text-primary" />
          ) : (
            <Loader2 className="size-4 animate-spin text-muted-foreground" />
          )}
          {heading(state, kind)}
        </DialogTitle>

        <DialogDescription>
          {state.error
            ? "Nothing was written unless the step below says otherwise."
            : state.done
              ? "The deployment is in your folder and registered here. Nothing is running yet — start it from the dashboard."
              : "Konstruktor is building, authorizing and writing your deployment."}
        </DialogDescription>

        {state.error && <Alert variant="destructive">{state.error}</Alert>}

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
      </DialogContent>
    </Dialog>
  );
};
