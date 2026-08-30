import { Dialog, DialogContent, DialogDescription, DialogTitle } from "../ui/dialog";
import { EngineSetupPanel } from "./EngineSetupPanel";

/**
 * The engine panel as a dialog, for the places that only have room for a warning: the
 * status dot in the menu bar and the banner on Home. The wizard and the dashboards show
 * the panel inline instead.
 */
export const EngineSetupDialog = ({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) => (
  <Dialog open={open} onOpenChange={onOpenChange}>
    <DialogContent className="bg-card max-w-2xl max-h-[85vh] overflow-y-auto">
      <DialogTitle>Container engine</DialogTitle>
      <DialogDescription>
        Konstruktor runs every deployment through Docker Compose. This is what it found
        on this machine, and what to do about it.
      </DialogDescription>
      <EngineSetupPanel />
    </DialogContent>
  </Dialog>
);
