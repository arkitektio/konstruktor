import { AlertTriangle, Check, ChevronsUpDown, GitBranch, Loader2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import * as api from "../../api";
import type { Checkout } from "../../api";
import { useAlerter } from "../../alerter/alerter-context";
import { Button } from "../../components/ui/button";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "../../components/ui/command";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "../../components/ui/popover";
import { cn } from "../../utils";

/**
 * The branches a dev hub's checkouts are on, and the way to move them.
 *
 * A dev hub clones each service's repository into `mounts/<service>` and bind-mounts it
 * over the image's workspace, so the branch checked out on disk *is* the code the
 * container runs. Switching it is therefore a first-class action on the deployment
 * rather than something to go and do in a terminal — but only the checkout moves. The
 * container goes on running whatever it imported at start until the stack is recreated,
 * which is what the note under a switched service says.
 *
 * There is no card: each picker sits on its service's tile, where the tag would be on an
 * ordinary hub. `deploymentCheckouts` answering with an empty list is the only "is this
 * a dev hub" question anything has to ask.
 */

/** What one checkout is, in a phrase — the same order of precedence the CLI uses. */
const describe = (checkout: Checkout): string => {
  if (checkout.error) return checkout.error;
  if (checkout.detached) return "detached HEAD";
  return checkout.branch ?? "unknown";
};

export const BranchPicker = ({
  path,
  checkout,
  onSwitched,
  compact = false,
}: {
  path: string;
  checkout: Checkout;
  onSwitched: (next: Checkout) => void;
  /** Inline in a service tile: no border, no chrome, just the branch and a caret. */
  compact?: boolean;
}) => {
  const { alert } = useAlerter();
  const [open, setOpen] = useState(false);
  const [branches, setBranches] = useState<string[] | undefined>();
  const [busy, setBusy] = useState(false);

  // Listing fetches from the remote, so it happens when the picker is opened rather than
  // on render — a dev hub has one of these per service, and eight fetches on every
  // dashboard poll would be a great deal of network for a list nobody asked to see.
  useEffect(() => {
    if (!open || branches) return;
    let cancelled = false;
    api
      .checkoutBranches(path, checkout.service)
      .then((names) => !cancelled && setBranches(names))
      .catch(() => !cancelled && setBranches([]));
    return () => {
      cancelled = true;
    };
  }, [open, branches, path, checkout.service]);

  const switchTo = async (branch: string) => {
    setOpen(false);
    setBusy(true);
    try {
      onSwitched(await api.switchCheckoutBranch(path, checkout.service, branch));
    } catch (error) {
      // Uncommitted work and a checkout the containers wrote into as root both land
      // here, and both are explained far better by git than by anything this could say.
      alert({
        error: `Could not switch ${checkout.service}`,
        message: typeof error === "string" ? error : String(error),
        subtitle: "The checkout was left where it was.",
      });
    } finally {
      setBusy(false);
    }
  };

  // A checkout git cannot read is a checkout git cannot switch — most often because the
  // containers wrote into it as root, which is the expected dev-hub failure. The reason
  // is shown rather than the word "unavailable" on its own, because it is the whole
  // instruction: nothing here can fix an ownership problem for the user.
  if (checkout.error) {
    return (
      <span className="text-xs text-muted-foreground text-right max-w-[24ch] truncate" title={checkout.error}>
        {checkout.error}
      </span>
    );
  }

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant={compact ? "ghost" : "outline"}
          size={compact ? "xs" : "sm"}
          role="combobox"
          disabled={busy}
          title={checkout.dirty ? "Uncommitted changes — a switch will be refused." : "Switch branch"}
          className={cn(
            "gap-1.5 font-mono text-xs",
            compact && "h-5 px-1 -ml-1 text-muted-foreground hover:text-foreground min-w-0",
            checkout.dirty && "text-warning",
          )}
        >
          {busy ? (
            <Loader2 className="size-3 animate-spin" />
          ) : (
            <GitBranch className="size-3" />
          )}
          <span className="truncate">{describe(checkout)}</span>
          {checkout.dirty && <AlertTriangle className="size-3" />}
          <ChevronsUpDown className="size-3 opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="p-0 w-64" align="end">
        <Command>
          <CommandInput placeholder="Find a branch…" />
          <CommandList>
            {branches === undefined ? (
              <div className="p-3 text-xs text-muted-foreground">
                Fetching from origin…
              </div>
            ) : (
              <>
                <CommandEmpty>No branch by that name.</CommandEmpty>
                <CommandGroup>
                  {branches.map((branch) => (
                    <CommandItem
                      key={branch}
                      value={branch}
                      onSelect={() => switchTo(branch)}
                      className="font-mono text-xs"
                    >
                      <Check
                        className={cn(
                          "size-3",
                          branch === checkout.branch ? "opacity-100" : "opacity-0"
                        )}
                      />
                      {branch}
                    </CommandItem>
                  ))}
                </CommandGroup>
              </>
            )}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
};

/** Reads a deployment's checkouts, and keeps them in step with what the user switches. */
export const useCheckouts = (path: string) => {
  const [checkouts, setCheckouts] = useState<Checkout[]>([]);

  const reload = useCallback(async () => {
    try {
      setCheckouts(await api.deploymentCheckouts(path));
    } catch (e) {
      // An unreadable profile is already reported by the page around this; a dev hub
      // card that simply does not appear is the right degradation here.
      console.error("Could not read the deployment's checkouts", e);
      setCheckouts([]);
    }
  }, [path]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const replace = useCallback(
    (next: Checkout) =>
      setCheckouts((previous) =>
        previous.map((c) => (c.service === next.service ? next : c))
      ),
    []
  );

  return { checkouts, reload, replace };
};
