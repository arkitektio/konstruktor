import { useEffect, useMemo, useState } from "react";

import * as api from "../../api";
import type {
  CreateEvent,
  DeploymentRecord,
  HubStatus,
  ServiceId,
  ServiceMeta,
  ServicePlan,
  ServicesOutcome,
} from "../../api";
import { Alert } from "../../components/ui/alert";
import { Button } from "../../components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../../components/ui/dialog";
import {
  CreateState,
  InstallPanel,
  StateIcon,
  emptyCreateState,
  reduceCreate,
} from "../deploy/InstallProgress";
import { ServiceRow, ServiceSections } from "../deploy/ServiceRows";

/**
 * Adding services to a hub that exists, and taking them out.
 *
 * The wizard's list, with what the hub runs now pre-ticked. A change is a re-authorization
 * — the coordination server has to know about a new service, and vouch for its key — so
 * applying it shows the same device code the connect screen does, and nothing is written
 * until somebody accepts it. Then the stack is restarted with the new set, unless asked
 * not to. A service taken out keeps its data: its database and buckets stay.
 */

/** What the selection changes against what runs, in catalog order. Exported for the test. */
export const serviceDiff = (
  current: ServiceId[],
  selected: ServiceId[],
  catalog: ServiceMeta[]
): { add: ServiceId[]; remove: ServiceId[] } => ({
  add: catalog
    .map((s) => s.id)
    .filter((id) => selected.includes(id) && !current.includes(id)),
  remove: catalog
    .map((s) => s.id)
    .filter((id) => current.includes(id) && !selected.includes(id)),
});

export const ServicesDialog = ({
  open,
  deployment,
  status,
  onOpenChange,
  onDone,
}: {
  open: boolean;
  deployment: DeploymentRecord;
  /** What the dashboard read: the services running now come from it. */
  status: HubStatus;
  onOpenChange: (open: boolean) => void;
  /** After a change went through — the dashboard reloads what it shows. */
  onDone: () => void;
}) => {
  const current = useMemo(() => status.services.map((s) => s.id), [status]);
  const [catalog, setCatalog] = useState<ServiceMeta[]>([]);
  const [selected, setSelected] = useState<ServiceId[]>(current);
  const [experimentalOpen, setExperimentalOpen] = useState(false);
  const [plan, setPlan] = useState<{ plan?: ServicePlan; error?: string }>();
  const [apply, setApply] = useState(true);
  const [run, setRun] = useState<CreateState>(emptyCreateState);
  const [outcome, setOutcome] = useState<ServicesOutcome>();

  useEffect(() => {
    api.serviceCatalog().then((all) => {
      // A service with no image to run would be a switch that changes nothing.
      const offered = all.filter((s) => s.emitted);
      setCatalog(offered);
      // A hub already running an experimental service shows the section open.
      setExperimentalOpen(offered.some((s) => s.experimental && current.includes(s.id)));
    });
  }, [current]);

  const diff = useMemo(
    () => serviceDiff(current, selected, catalog),
    [current, selected, catalog]
  );
  const changed = diff.add.length + diff.remove.length > 0;
  const diffKey = `${diff.add.join(",")}|${diff.remove.join(",")}`;

  // The core's verdict on the change, before anybody is sent to a browser: what it would
  // do, and why it cannot — Rekuest while hooked services run, the last service.
  useEffect(() => {
    if (!changed) {
      setPlan(undefined);
      return;
    }
    let cancelled = false;
    api
      .planServiceChange(deployment.path, diff.add, diff.remove)
      .then((next) => !cancelled && setPlan({ plan: next }))
      .catch((error) => !cancelled && setPlan({ error: String(error) }));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deployment.path, diffKey]);

  const toggle = (service: ServiceMeta) =>
    setSelected((previous) =>
      previous.includes(service.id)
        ? previous.filter((id) => id !== service.id)
        : [...previous, service.id]
    );

  const nameOf = (id: ServiceId) => catalog.find((s) => s.id === id)?.name ?? id;
  const names = (ids: ServiceId[]) => ids.map(nameOf).join(", ");

  const started = run.running || run.done || run.error !== null;

  const go = async () => {
    setRun({ ...emptyCreateState, running: true });
    const onEvent = (event: CreateEvent) =>
      setRun((previous) => reduceCreate(previous, event));
    try {
      const result = await api.changeServices(
        { path: deployment.path, add: diff.add, remove: diff.remove, apply },
        onEvent
      );
      setOutcome(result);
      setRun((previous) => ({ ...previous, running: false, done: true }));
      onDone();
    } catch (error) {
      setRun((previous) => ({
        ...previous,
        running: false,
        error: typeof error === "string" ? error : String(error),
      }));
    }
  };

  const stopWaiting = async () => {
    setRun((previous) => ({ ...previous, cancelled: true }));
    await api.cancelAuthorization();
  };

  const title = !started
    ? "Manage services"
    : run.cancelled && !run.done
      ? "Stopped"
      : run.error
        ? "That did not work"
        : run.done
          ? "Services changed"
          : run.staged
            ? "Waiting to be accepted"
            : run.event?.event === "starting" || run.event?.event === "log"
              ? "Restarting the stack…"
              : "Changing services…";

  return (
    <Dialog
      open={open}
      // Not while it runs: the code on screen is the only way to accept it.
      onOpenChange={(next) => !run.running && onOpenChange(next)}
    >
      <DialogContent className="bg-card max-w-2xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {started && <StateIcon state={run} />}
            {title}
          </DialogTitle>
          <DialogDescription>
            {started
              ? run.cancelled && !run.done
                ? "Nothing was written — the change was never accepted."
                : "The coordination server has to accept the new set of services before anything is written."
              : `What ${deployment.name} runs. The ones highlighted are in the hub; click one to add or remove it.`}
          </DialogDescription>
        </DialogHeader>

        {!started && (
          <div className="flex flex-col gap-4">
            <div className="max-h-[45vh] overflow-y-auto pr-1">
              <ServiceSections
                services={catalog}
                experimentalOpen={experimentalOpen}
                onExperimentalOpenChange={setExperimentalOpen}
                row={(service) => (
                  <ServiceRow
                    key={service.id}
                    service={service}
                    on={selected.includes(service.id)}
                    note={
                      diff.add.includes(service.id)
                        ? "adding"
                        : diff.remove.includes(service.id)
                          ? "removing"
                          : undefined
                    }
                    onClick={() => toggle(service)}
                  />
                )}
              />
            </div>

            {changed && (
              <div className="flex flex-col gap-2 text-sm" data-testid="service-diff">
                {diff.add.length > 0 && (
                  <div className="font-medium">{`Adding ${names(diff.add)}`}</div>
                )}
                {diff.remove.length > 0 && (
                  <div>
                    <span className="font-medium">{`Removing ${names(diff.remove)}`}</span>{" "}
                    <span className="text-muted-foreground">
                      — their data is kept: the databases and buckets stay, and adding
                      one back picks them up again.
                    </span>
                  </div>
                )}
                {plan?.error && <Alert variant="destructive">{plan.error}</Alert>}
                {plan?.plan?.notes
                  // The kept-data note is said above already.
                  .filter((note) => !note.startsWith("The data of"))
                  .map((note) => (
                    <div key={note} className="text-xs text-muted-foreground">
                      {note}
                    </div>
                  ))}
                <label className="flex items-start gap-2.5 text-sm cursor-pointer mt-1">
                  <input
                    type="checkbox"
                    className="mt-1"
                    checked={apply}
                    onChange={(event) => setApply(event.target.checked)}
                  />
                  <span>
                    Restart the stack with the new services once accepted
                    <span className="block text-xs text-muted-foreground">
                      Creates their databases, starts the new containers and removes the
                      ones taken out. Unticked, only the files are rewritten.
                    </span>
                  </span>
                </label>
              </div>
            )}
          </div>
        )}

        {started && (
          <div className="flex flex-col gap-3">
            <InstallPanel state={run} onCancel={stopWaiting} />
            {run.done && outcome && (
              <Alert className="border-primary/50">
                <div className="flex flex-col gap-1">
                  <span>
                    {deployment.name} now runs {names(outcome.plan.services)}.
                  </span>
                  {!outcome.applied && (
                    <span>
                      The files are rewritten; restart the stack from the dashboard to
                      pick them up.
                    </span>
                  )}
                  {outcome.mesh_granted && !outcome.applied && (
                    <strong>
                      A mesh key came with it. It expires 15 minutes after it was issued —
                      restart the stack before then.
                    </strong>
                  )}
                </div>
              </Alert>
            )}
          </div>
        )}

        <DialogFooter>
          {!started && (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)}>
                Cancel
              </Button>
              <Button disabled={!changed || !plan?.plan} onClick={() => void go()}>
                Apply changes
              </Button>
            </>
          )}
          {started && !run.running && (
            <Button onClick={() => onOpenChange(false)}>Close</Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
