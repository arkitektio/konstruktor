import { AlertTriangle, Check, ChevronsUpDown, GitBranch, Loader2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import * as api from "../../api";
import type { Checkout } from "../../api";
import { useAlerter } from "../../alerter/alerter-context";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../components/ui/card";
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
 * The whole card is absent on an ordinary hub: `deploymentCheckouts` answers with an
 * empty list, which is the only "is this a dev hub" question anything has to ask.
 */

/** What one checkout is, in a phrase — the same order of precedence the CLI uses. */
const describe = (checkout: Checkout): string => {
  if (checkout.error) return checkout.error;
  if (checkout.detached) return "detached HEAD";
  return checkout.branch ?? "unknown";
};

const BranchPicker = ({
  path,
  checkout,
  onSwitched,
}: {
  path: string;
  checkout: Checkout;
  onSwitched: (next: Checkout) => void;
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
          variant="outline"
          size="sm"
          role="combobox"
          disabled={busy}
          className="gap-1.5 font-mono text-xs"
        >
          {busy ? (
            <Loader2 className="size-3 animate-spin" />
          ) : (
            <GitBranch className="size-3" />
          )}
          {describe(checkout)}
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

export const CheckoutsCard = ({
  path,
  checkouts,
  onChanged,
}: {
  path: string;
  checkouts: Checkout[];
  /** A checkout moved — the caller re-reads, and shows that a recreate is due. */
  onChanged: (next: Checkout) => void;
}) => (
  <Card>
    <CardHeader>
      <CardTitle className="flex items-center gap-3">
        <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted">
          <GitBranch className="size-4" />
        </span>
        Source checkouts
        <Badge variant="outline">dev hub</Badge>
      </CardTitle>
      <CardDescription>
        Each service runs the code in <code>mounts/</code> rather than the code baked into
        its image, so the branch checked out here is the branch that runs. Switching one
        is refused over uncommitted changes — commit or stash them first. Recreate the
        stack afterwards for the containers to pick the new code up.
      </CardDescription>
    </CardHeader>
    <CardContent className="flex flex-col gap-2">
      {checkouts.map((checkout) => (
        <div
          key={checkout.service}
          className="flex items-center justify-between gap-2 text-xs"
        >
          <span className="flex items-center gap-2 min-w-0">
            <span className="truncate" title={checkout.repo}>
              {checkout.service}
            </span>
            {checkout.dirty && (
              <Badge
                variant="outline"
                className="gap-1 font-normal border-warning/60 text-warning"
                title="Tracked files differ from HEAD. Untracked files are ignored — the containers write into this folder."
              >
                <AlertTriangle className="size-3" />
                uncommitted
              </Badge>
            )}
            {checkout.head && !checkout.error && (
              <span className="text-muted-foreground font-mono">{checkout.head}</span>
            )}
          </span>
          <BranchPicker path={path} checkout={checkout} onSwitched={onChanged} />
        </div>
      ))}
    </CardContent>
  </Card>
);

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
