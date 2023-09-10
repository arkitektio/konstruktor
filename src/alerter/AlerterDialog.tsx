import { DialogTitle } from "@radix-ui/react-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
} from "../components/ui/dialog";
import { useAlerter } from "./alerter-context";
import { useEffect } from "react";
import React from "react";

export const AlerterDialog = () => {
  const { activeError, ack } = useAlerter();

  const [open, setOpen] = React.useState(false);

  useEffect(() => {
    setOpen(activeError != null);
  }, [activeError]);

  return (
    <>
      <Dialog open={open} onOpenChange={ack}>
        <DialogContent>
          <DialogTitle>{activeError?.error}</DialogTitle>
          <DialogDescription>{activeError?.message}</DialogDescription>
          <DialogDescription>{activeError?.subtitle}</DialogDescription>
        </DialogContent>
      </Dialog>
    </>
  );
};
