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
export type CreateState = {
  running: boolean;
  /** The last event, which is what decides the heading. */
  event?: CreateEvent;
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

const heading = (state: CreateState): string => {
  if (state.error) return "That did not work";
  if (state.done) return "Your hub is ready";

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
    case "starting":
    case "log":
      return "Starting the stack…";
    default:
      return "Creating your hub";
  }
};

export const InstallProgress = ({
  open,
  state,
  onClose,
}: {
  open: boolean;
  state: CreateState;
  onClose: () => void;
}) => {
  const finished = state.done || state.error !== null;
  const staged = state.event?.event === "staged" ? state.event : undefined;
  const waiting = state.event?.event === "waiting" ? state.event : undefined;

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
          {heading(state)}
        </DialogTitle>

        <DialogDescription>
          {state.error
            ? "Nothing was written unless the step below says otherwise."
            : state.done
              ? "The deployment is in your folder and registered here."
              : "Konstruktor is building, authorizing and writing your deployment."}
        </DialogDescription>

        {state.error && <Alert variant="destructive">{state.error}</Alert>}

        {/* The one part a person has to act on. */}
        {staged && !state.done && !state.error && (
          <div className="rounded-lg border border-primary/60 bg-primary/5 p-4 flex flex-col gap-3">
            <div className="text-sm">
              Somebody with an account on your coordination server has to accept this hub.
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
