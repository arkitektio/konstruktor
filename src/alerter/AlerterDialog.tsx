import { DialogTitle } from "@/components/ui/dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
} from "../components/ui/dialog";
import { ComposeLog } from "../components/ComposeLog";
import { useAlerter } from "./alerter-context";
import { useEffect } from "react";
import React from "react";

export const AlerterDialog = () => {
  const { activeError, ack } = useAlerter();

  const [open, setOpen] = React.useState(false);

  useEffect(() => {
    setOpen(activeError != null);
  }, [activeError]);

  const log = activeError?.log;

  return (
    <>
      <Dialog open={open} onOpenChange={ack}>
        <DialogContent className={log?.length ? "sm:max-w-2xl" : undefined}>
          <DialogTitle>{activeError?.error}</DialogTitle>
          {/* Multi-line messages are command output: keep their lines. */}
          <DialogDescription className="whitespace-pre-wrap break-words">
            {activeError?.message}
          </DialogDescription>
          <DialogDescription>{activeError?.subtitle}</DialogDescription>
          {log && log.length > 0 && (
            <div className="min-w-0">
              <div className="text-xs font-medium text-muted-foreground mb-1">Output</div>
              <ComposeLog entries={log} />
            </div>
          )}
        </DialogContent>
      </Dialog>
    </>
  );
};
