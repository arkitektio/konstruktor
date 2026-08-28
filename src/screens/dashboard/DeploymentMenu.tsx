import { open } from "@tauri-apps/plugin-shell";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  Download,
  ExternalLink,
  EyeOff,
  FolderOpen,
  MoreHorizontal,
  RefreshCw,
  ScrollText,
  ShieldCheck,
  Trash2,
} from "lucide-react";

import {
  ConfirmByNameDialog,
  ConfirmDialog,
  useComposeAction,
} from "../../CommandButton";
import { Button } from "../../components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "../../components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "../../components/ui/tooltip";
import * as api from "../../api";
import type { DeletionPlan, DeploymentRecord } from "../../api";
import { useRegistry } from "../../registry/registry-context";

/**
 * Everything you can do to a deployment that is not "start it" or "stop it".
 *
 * These used to be scattered over three places — ghost buttons in the app bar, a button
 * in the footer, and a "Danger zone" block at the bottom of a page you had to scroll to
 * reach. They are one menu now, sitting next to the header, with the destructive three
 * behind a separator at the end. Start and Stop stay as real footer buttons: they are the
 * two things people actually come here to click.
 */

/** Which confirmation is open, if any. Each one names a different amount of loss. */
type Confirm = "down" | "purge" | "forget" | "delete";

/**
 * The two confirmations that are a line of copy and a button.
 *
 * `delete` and `purge` are not here: both are irreversible, both ask for the hub's name to
 * be typed, and both need a description richer than a string.
 */
const CONFIRM_COPY: Record<
  Exclude<Confirm, "delete" | "purge">,
  { title: string; description: string; action: string }
> = {
  down: {
    title: "Remove the containers?",
    description:
      "Stops and removes the containers and networks. The database and object storage stay on disk in the deployment folder, so starting it again picks up where it left off.",
    action: "Remove",
  },
  forget: {
    title: "Forget this deployment?",
    description:
      "Only removes it from this list. The folder, its configuration, its data and its containers are all left exactly as they are — this deletes nothing. To actually remove a hub, use Delete hub.",
    action: "Forget",
  },
};

export const DeploymentMenu = ({
  deployment,
  /** The gateway, when the profile could be read. Absent hides the "open" item. */
  url,
  onRefresh,
  onReload,
}: {
  deployment: DeploymentRecord;
  url?: string;
  onRefresh: () => void;
  onReload: () => void;
}) => {
  const navigate = useNavigate();
  const { forget, refresh } = useRegistry();
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  /**
   * What a delete would take with it, asked for only once the dialog is open.
   *
   * The core works it out from the folder — the checkouts under `mounts/` are not in the
   * registry — so it is a round trip, and there is no reason to make it on every render
   * of a menu nobody has opened.
   */
  const [plan, setPlan] = useState<DeletionPlan | undefined>();

  useEffect(() => {
    if (confirm !== "delete" && confirm !== "purge") return;
    let cancelled = false;
    api
      .planDeletion(deployment.id)
      .then((next) => !cancelled && setPlan(next))
      // A plan that cannot be built is not worth an alert of its own: the delete itself
      // fails on the same guard, in the dialog, where the user is already looking.
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [confirm, deployment.id]);

  /**
   * Pulling lives here rather than on the dashboard because the updates card that used to
   * carry it is only rendered when something is already waiting — and a pull is how you
   * find out whether anything is.
   */
  const pull = useComposeAction({
    path: deployment.path,
    action: "pull",
    title: "Pull images",
    callback: onRefresh,
  });

  const down = useComposeAction({
    path: deployment.path,
    action: "down",
    title: "Remove containers",
    callback: onRefresh,
  });
  const run = async () => {
    if (confirm === "down") await down.run();
    if (confirm === "forget") {
      // Away from this screen first. `forget` reloads the registry, and a reload while
      // the dashboard is still mounted leaves it looking up a deployment that is no
      // longer listed — which renders "Unknown deployment" for a frame.
      navigate("/");
      await forget(deployment.id);
    }
  };

  const copy =
    confirm === "down" || confirm === "forget" ? CONFIRM_COPY[confirm] : undefined;

  return (
    <>
      {/*
        Not modal: a modal menu takes the body's pointer events for itself and hands them
        back on a timer as it closes, which collides with the confirmation dialog opening
        in the same tick and can leave the page unclickable. Nothing here needs a focus
        trap, so the menu simply does not take one.
      */}
      <DropdownMenu modal={false}>
        <Tooltip>
          <TooltipTrigger asChild>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="icon-sm" aria-label="Deployment actions">
                <MoreHorizontal className="size-4" />
              </Button>
            </DropdownMenuTrigger>
          </TooltipTrigger>
          <TooltipContent>Deployment actions</TooltipContent>
        </Tooltip>

        <DropdownMenuContent className="min-w-56">
          {url && (
            <DropdownMenuItem onSelect={() => open(url)}>
              <ExternalLink />
              Open in browser
            </DropdownMenuItem>
          )}
          <DropdownMenuItem onSelect={() => open(deployment.path)}>
            <FolderOpen />
            Open folder
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => navigate(`/logs/${deployment.id}`)}>
            <ScrollText />
            Logs
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={onReload}>
            <RefreshCw />
            Reload
          </DropdownMenuItem>
          <DropdownMenuItem
            disabled={pull.running}
            // The pull outlives the menu, which closes on select; the dashboard reloads
            // from the callback when it finishes.
            onSelect={() => void pull.run()}
          >
            <Download />
            {pull.running ? "Pulling…" : "Pull images"}
          </DropdownMenuItem>

          {deployment.kind === "hub" && (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={() => navigate(`/connect/${deployment.id}`)}>
                <ShieldCheck />
                Authorize
              </DropdownMenuItem>
            </>
          )}

          <DropdownMenuSeparator />
          <DropdownMenuLabel>Danger zone</DropdownMenuLabel>
          <DropdownMenuItem variant="destructive" onSelect={() => setConfirm("down")}>
            <Trash2 />
            Remove containers
          </DropdownMenuItem>
          <DropdownMenuItem
            variant="destructive"
            onSelect={() => setConfirm("purge")}
          >
            <Trash2 />
            Delete all data
          </DropdownMenuItem>
          <DropdownMenuItem variant="destructive" onSelect={() => setConfirm("forget")}>
            <EyeOff />
            Forget deployment
          </DropdownMenuItem>
          <DropdownMenuItem variant="destructive" onSelect={() => setConfirm("delete")}>
            <Trash2 />
            Delete hub
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      {confirm === "purge" && (
        <ConfirmByNameDialog
          open
          onOpenChange={(next) => {
            if (!next) {
              setConfirm(null);
              setPlan(undefined);
            }
          }}
          title="Delete all data?"
          expected={deployment.name}
          confirmTitle="Delete the data"
          runningTitle="Deleting…"
          description={
            <>
              <span>
                Stops and removes the containers, then deletes the database and the object
                storage — every file, image and account in this hub — for good.
              </span>
              {plan && plan.data_dirs.length > 0 && (
                <span className="font-mono text-xs break-all">
                  {plan.data_dirs.join("\n")}
                </span>
              )}
              <span>
                The folder stays, along with its configuration, credentials and{" "}
                <code>docker-compose.yaml</code>, so you can start it again empty.
              </span>
              {plan?.skipped.map((skip) => (
                <span key={skip.mount}>
                  This hub also keeps data at{" "}
                  <span className="font-mono break-all">{skip.mount}</span>, which
                  Konstruktor will not touch — {skip.explanation}. Remove it yourself if
                  you want it gone.
                </span>
              ))}
              {plan?.on_a_mesh && (
                <span>
                  Its place on the mesh goes too: the tailnet state is a volume, and the
                  key that joined it was single-use, so the hub has to be authorized again
                  to rejoin.
                </span>
              )}
            </>
          }
          onConfirm={async () => {
            await api.purgeDeploymentData(deployment.id);
            onRefresh();
          }}
        />
      )}

      {confirm === "delete" && (
        <ConfirmByNameDialog
          open
          onOpenChange={(next) => {
            if (!next) {
              setConfirm(null);
              setPlan(undefined);
            }
          }}
          title={`Delete ${deployment.name}?`}
          expected={deployment.name}
          confirmTitle="Delete this hub"
          runningTitle="Deleting…"
          description={
            <>
              <span>
                This removes the containers and the networks, and then the folder itself,
                at <span className="font-mono break-all">{deployment.path}</span> —
                including the database and object storage kept inside it. Nothing is left
                behind and none of it can be undone.
              </span>
              {plan?.skipped.map((skip) => (
                <span key={skip.mount}>
                  It keeps data at{" "}
                  <span className="font-mono break-all">{skip.mount}</span> as well, which
                  is outside the folder and will not be removed — {skip.explanation}.
                </span>
              ))}
              {plan && plan.checkouts.length > 0 && (
                <span>
                  It also deletes the source checkouts under <code>mounts/</code> (
                  {plan.checkouts.join(", ")}). Anything committed nowhere else goes with
                  them.
                </span>
              )}
              {plan?.was_authorized && (
                <span>
                  The registration on the coordination server is not this app's to
                  withdraw, and stays until it is removed there.
                </span>
              )}
              <span>
                The images stay on this machine — they are shared with other deployments.
              </span>
            </>
          }
          onConfirm={async () => {
            // Awaited, so a failure reaches the dialog rather than a screen the user has
            // already been sent away from. Only once it succeeds does the dashboard go —
            // and it goes before the registry reloads, for the reason `forget` does.
            await api.deleteDeployment(deployment.id);
            navigate("/");
            void refresh();
          }}
        />
      )}

      {copy && (
        <ConfirmDialog
          open
          onOpenChange={(next) => !next && setConfirm(null)}
          title={copy.title}
          description={copy.description}
          confirmTitle={copy.action}
          onConfirm={() => void run()}
        />
      )}
    </>
  );
};
