import { Boxes, ChevronDown, GitBranch, Plus, Settings2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useController, useFormContext, useWatch } from "react-hook-form";
import { useCommunication } from "../../../communication/communication-context";
import { Button } from "../../../components/ui/button";
import { Card } from "../../../components/ui/card";
import { Input } from "../../../components/ui/input";
import { ErrorDisplay } from "../../../components/Error";
import { cn } from "../../../utils";
import * as api from "../../../api";
import type { ServiceId, ServiceMeta } from "../../../api";
import { emptyOverride, type ServiceOverride } from "../hub-form";
import { AdvancedFields, StepFrame } from "../../wizard/StepFrame";

/**
 * Which services this hub runs, and — since the provenance step folded into it — where
 * Rekuest comes from.
 *
 * The list is the left column and what a service is *for* is the right one: the names
 * are jargon, and a grid of one-liners asked people to pick between eight things they
 * could not tell apart. Clicking a card does both jobs: it takes the service in or out,
 * and it puts the service in the panel so the paragraph follows the pointer's last
 * deliberate action rather than wherever it happens to rest.
 */

/**
 * Rekuest is not a tick like the others. It is either part of this deployment, or it is
 * somewhere else and everything here trusts it, or there is none at all — and the core
 * reads exactly those three out of the one `rekuest_server` string.
 *
 * The fourth state is this wizard's own: empty means "taken out of the hub and not yet
 * told what to trust instead". It is deliberately not valid, which is what keeps Next
 * away until the question on the right has an answer.
 */
type RekuestChoice = "local" | "remote" | "none" | "asking";

const choiceOf = (server: string): RekuestChoice => {
  const value = server.trim();
  if (value === "local") return "local";
  if (value === "none") return "none";
  if (value === "") return "asking";
  return "remote";
};

export const ServicesStep = () => {
  const { field, fieldState } = useController<Record<string, ServiceId[]>>({
    name: "services",
  });
  const { getValues, setValue } = useFormContext();
  const rekuestServer = (useWatch({ name: "rekuestServer" }) as string) ?? "local";
  const overrides = (useWatch({ name: "serviceOptions" }) ??
    {}) as Partial<Record<ServiceId, ServiceOverride>>;
  const { git } = useCommunication();
  const hasGit = git?.cli ?? false;
  const [services, setServices] = useState<ServiceMeta[]>([]);
  const [active, setActive] = useState<ServiceId | null>(null);
  /** Whether the gear's fields are showing, which the gear in the list also opens. */
  const [gearOpen, setGearOpen] = useState(false);

  // The catalog — names, descriptions, which are pre-ticked — is published by the core,
  // so the wizard's list and the CLI's `--services` help cannot drift apart.
  useEffect(() => {
    api.serviceCatalog().then((catalog) => {
      setServices(catalog);
      setActive((current) => current ?? catalog[0]?.id ?? null);
      if ((field.value ?? []).length === 0) {
        field.onChange(catalog.filter((s) => s.default).map((s) => s.id));
      }
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Source mode is the one answer here that needs something off this machine. A probe
  // that comes back without git — the user went back to the first step and rechecked —
  // must not leave a hub standing that cannot be cloned into. Same rule the Advanced
  // step used to apply to the deployment-wide dev hub.
  useEffect(() => {
    if (git?.cli !== false) return;
    // Read through `getValues` rather than the watched object: `useWatch` hands back a
    // new identity on every keystroke anywhere in the form, and an effect that depends
    // on it writes, re-renders and revalidates the whole wizard on each one.
    const current = (getValues("serviceOptions") ?? {}) as Partial<
      Record<ServiceId, ServiceOverride>
    >;
    if (!Object.values(current).some((asked) => asked?.fromSource)) return;
    setValue(
      "serviceOptions",
      Object.fromEntries(
        Object.entries(current).map(([id, asked]) => [
          id,
          { ...asked, fromSource: false },
        ])
      ),
      { shouldValidate: true }
    );
  }, [git, getValues, setValue]);

  const setOverride = (id: ServiceId, next: Partial<ServiceOverride>) =>
    setValue(
      "serviceOptions",
      {
        ...overrides,
        [id]: { ...emptyOverride, ...overrides[id], ...next },
      },
      { shouldValidate: true }
    );

  const choice = choiceOf(rekuestServer);
  const selected = new Set(field.value ?? []);

  const isOn = (service: ServiceMeta) =>
    service.id === "rekuest"
      ? choice === "local"
      : service.emitted && selected.has(service.id);

  const toggle = (service: ServiceMeta) => {
    setActive(service.id);


    // No image, nothing to switch on: the panel explains it, the card does not pretend.
    if (!service.emitted) return;

    if (service.id === "rekuest") {
      // Off means "not here", which is a question rather than an answer — the panel asks
      // it. Until it is answered the field is empty, and the step does not validate.
      setValue("rekuestServer", choice === "local" ? "" : "local", {
        shouldValidate: true,
      });
      return;
    }

    const next = new Set(selected);
    if (next.has(service.id)) next.delete(service.id);
    else next.add(service.id);
    field.onChange(services.filter((s) => next.has(s.id)).map((s) => s.id));
  };

  const shown = services.find((service) => service.id === active);

  return (
    <StepFrame
      icon={Boxes}
      title="Services"
      subtitle="What should this run?"
      lead="Each service is a separate container behind the gateway, and each one is registered with the coordination server under this hub."
      className="max-w-5xl"
    >
      <div className="grid grid-cols-1 @3xl:grid-cols-[minmax(0,20rem)_minmax(0,1fr)] gap-4 items-start">
        <div className="flex flex-col gap-1.5">
          {services.map((service) => (
            <ServiceRow
              key={service.id}
              service={service}
              on={isOn(service)}
              active={service.id === active}
              fromSource={overrides[service.id]?.fromSource ?? false}
              onClick={() => toggle(service)}
              onGear={() => {
                setActive(service.id);
                setGearOpen(true);
              }}
            />
          ))}
        </div>

        <div className="@3xl:sticky @3xl:top-4">
          {shown && (
            <Panel
              service={shown}
              on={isOn(shown)}
              onToggle={() => toggle(shown)}
              gear={{
                open: gearOpen,
                toggle: () => setGearOpen((open) => !open),
                hasGit,
                override: overrides[shown.id] ?? emptyOverride,
                set: (next: Partial<ServiceOverride>) => setOverride(shown.id, next),
              }}
              rekuest={{
                choice,
                server: rekuestServer,
                set: (value: string) =>
                  setValue("rekuestServer", value, { shouldValidate: true }),
              }}
            />
          )}
        </div>
      </div>

      {fieldState.error && (
        <div className="max-w-xl mt-3">
          <ErrorDisplay name="services" />
        </div>
      )}
      <ErrorDisplay name="rekuestServer" className="mt-3" />
      <ErrorDisplay name="serviceOptions" className="mt-3" />
    </StepFrame>
  );
};

/**
 * One service in the list. Being in the hub is said by the highlight alone — a checkbox
 * next to a card that already changes colour was two controls for one bit, and the tick
 * drew the eye to the wrong thing.
 */
const ServiceRow = ({
  service,
  on,
  active,
  fromSource,
  onClick,
  onGear,
}: {
  service: ServiceMeta;
  on: boolean;
  active: boolean;
  /** Running from a checkout, which is worth seeing without opening the gear. */
  fromSource: boolean;
  onClick: () => void;
  onGear: () => void;
}) => (
  <Card
    onClick={onClick}
    className={cn(
      "gap-0 py-2.5 px-3 border cursor-pointer transition-colors",
      // In the hub, or not: the highlight is the whole statement.
      on
        ? "border-primary bg-primary/5 font-medium"
        : "border-border text-muted-foreground",
      // Being read about is a different thing from being in, and has to be legible on
      // top of either — hence a ring rather than another shade of the same colour.
      active && "ring-1 ring-foreground/20"
    )}
  >
    <div className="flex items-center gap-2">
      <span className="truncate">{service.name}</span>
      {fromSource && (
        <GitBranch className="size-3.5 shrink-0 text-primary" aria-label="from source" />
      )}
      {!service.emitted && (
        <span className="text-[10px] uppercase tracking-wide text-muted-foreground">
          soon
        </span>
      )}
      {service.emitted && (
        <button
          type="button"
          aria-label={`${service.name} settings`}
          // Not the card's click: the gear is for the settings of a service, which is a
          // different question from whether the hub runs it at all.
          onClick={(event) => {
            event.stopPropagation();
            onGear();
          }}
          className="ml-auto -mr-1 p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
        >
          <Settings2 className="size-3.5" />
        </button>
      )}
    </div>
  </Card>
);

/** The right-hand column: what this one is for, and the single control that adds it. */
type Gear = {
  open: boolean;
  toggle: () => void;
  hasGit: boolean;
  override: ServiceOverride;
  set: (next: Partial<ServiceOverride>) => void;
};

const Panel = ({
  service,
  on,
  onToggle,
  gear,
  rekuest,
}: {
  service: ServiceMeta;
  on: boolean;
  onToggle: () => void;
  gear: Gear;
  rekuest: {
    choice: RekuestChoice;
    server: string;
    set: (value: string) => void;
  };
}) => (
  <Card className="gap-0 py-5 px-5 border border-border">
    <div className="flex items-start gap-3">
      <div className="min-w-0 flex-1">
        <div className="text-lg font-semibold">{service.name}</div>
        <div className="text-sm text-muted-foreground">{service.description}</div>
      </div>
      {service.emitted && (
        <Button size="sm" variant={on ? "outline" : "default"} onClick={onToggle}>
          {on ? (
            "Remove"
          ) : (
            <>
              <Plus className="size-3.5" />
              Add
            </>
          )}
        </Button>
      )}
    </div>

    <p className="text-sm leading-relaxed mt-4">{service.purpose}</p>

    {!service.emitted && (
      <p className="text-xs text-muted-foreground mt-4">
        No image is published yet, so this cannot be switched on — it would not change
        the generated stack.
      </p>
    )}

    {service.id === "rekuest" && <RekuestChoiceBlock {...rekuest} />}

    {service.emitted && <GearFields service={service} gear={gear} />}
  </Card>
);

/**
 * The gear: what this one service does differently from the rest of the hub.
 *
 * Ordered by who needs it. The settings that change what the service *does* come first —
 * where Alpaka's models come from, which apps Kabinet offers — and the ones only a
 * developer wants sit under "Advanced", collapsed. Running from a checkout used to be the
 * whole of this panel, which put a git question in front of everybody who opened it to
 * change something ordinary.
 */
const GearFields = ({ service, gear }: { service: ServiceMeta; gear: Gear }) => (
  <div className="mt-5 border-t border-border pt-3">
    <button
      type="button"
      onClick={gear.toggle}
      className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
    >
      <Settings2 className="size-3.5" />
      {service.name} settings
      <ChevronDown
        className={cn("size-3.5 transition-transform", gear.open && "rotate-180")}
      />
    </button>

    {gear.open && (
      <div className="mt-3 flex flex-col gap-4">
        {service.id === "alpaka" && <OllamaFields gear={gear} />}
        {service.id === "kabinet" && <RepositoryFields gear={gear} />}

        <AdvancedFields label="Advanced">
          <label className="flex items-start gap-2.5 text-sm cursor-pointer">
            <input
              type="checkbox"
              className="mt-1"
              checked={gear.override.debug}
              onChange={(event) => gear.set({ debug: event.target.checked })}
            />
            <span className="min-w-0">
              <span className="font-medium">Debug mode</span>
              <span className="block text-xs text-muted-foreground mt-0.5">
                Django's debug mode for {service.name} alone: full tracebacks in
                responses instead of a bare 500. Useful while something is wrong, and
                worth turning off again — it shows internals to anyone who can reach the
                service.
              </span>
            </span>
          </label>

          <SourceFields service={service} gear={gear} />
        </AdvancedFields>
      </div>
    )}
  </div>
);

/**
 * Where Alpaka gets its models.
 *
 * Worth asking rather than assuming: the profile has always said `ollama_config: local`,
 * but nothing started an Ollama and no generated config pointed at one, so a stock Alpaka
 * had no provider at all. Answering here is what makes the stack match the profile.
 */
const OllamaFields = ({ gear }: { gear: Gear }) => {
  const { ollama, ollamaUrl } = gear.override;

  return (
    <div className="flex flex-col gap-2">
      <div className="text-sm font-medium">Language models</div>
      <div className="text-xs text-muted-foreground -mt-1">
        Alpaka needs somewhere to run models. Without one it starts, but answers nothing.
      </div>

      <Card
        onClick={() => gear.set({ ollama: ollama === "local" ? "" : "local" })}
        className={cn(
          "gap-1 py-3 px-4 cursor-pointer border transition-colors",
          ollama === "local" ? "border-primary bg-primary/5" : "border-border"
        )}
      >
        <div className="font-medium text-sm">Run Ollama here</div>
        <div className="text-xs text-muted-foreground">
          Adds an Ollama container to this deployment, reachable by the other services
          but not published outside it. No model is downloaded now — they are gigabytes
          each, are pulled on demand, and are kept in a volume so a restart does not
          fetch them again.
        </div>
      </Card>

      <Card
        onClick={() => gear.set({ ollama: "remote" })}
        className={cn(
          "gap-1 py-3 px-4 cursor-pointer border transition-colors",
          ollama === "remote" ? "border-primary bg-primary/5" : "border-border"
        )}
      >
        <div className="font-medium text-sm">Use one that already exists</div>
        <div className="text-xs text-muted-foreground">
          Point at an Ollama on another machine — the one with the GPU, usually.
        </div>
        {ollama === "remote" && (
          <Input
            autoFocus
            value={ollamaUrl}
            onChange={(event) => gear.set({ ollamaUrl: event.target.value })}
            onClick={(event) => event.stopPropagation()}
            placeholder="gpu-box.lab:11434"
            autoComplete="off"
            spellCheck={false}
            className="h-9 mt-2"
          />
        )}
      </Card>
    </div>
  );
};

/** Which analysis apps Kabinet offers once the hub is up. */
const RepositoryFields = ({ gear }: { gear: Gear }) => (
  <div className="flex flex-col gap-1.5">
    <div className="text-sm font-medium">App repositories</div>
    <div className="text-xs text-muted-foreground">
      One per line. These are offered in Kabinet's app store when the hub starts. Left
      empty, the two Arkitekt ships with are used.
    </div>
    <textarea
      value={gear.override.repositories}
      onChange={(event) => gear.set({ repositories: event.target.value })}
      rows={3}
      spellCheck={false}
      autoComplete="off"
      placeholder={"jhnnsrs/ome:main\njhnnsrs/renderer:main"}
      className="w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm font-mono shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
    />
  </div>
);

/** Running this one service from a checkout of its source. Needs git on this machine. */
const SourceFields = ({ service, gear }: { service: ServiceMeta; gear: Gear }) => {
  const { fromSource, branch } = gear.override;

  return (
    <div className="flex flex-col gap-3">
      <label
        className={cn(
          "flex items-start gap-2.5 text-sm",
          gear.hasGit ? "cursor-pointer" : "cursor-not-allowed opacity-70"
        )}
      >
        <input
          type="checkbox"
          className="mt-1"
          checked={fromSource}
          disabled={!gear.hasGit}
          onChange={(event) => gear.set({ fromSource: event.target.checked })}
        />
        <span className="min-w-0">
          <span className="font-medium">Run from source</span>
          <span className="block text-xs text-muted-foreground mt-0.5">
            Clone {service.name}'s repository into mounts/ and mount it over the image's
            workspace, so the container runs the code on this machine. The image still
            supplies the interpreter and the dependencies.
          </span>
          {!gear.hasGit && (
            <span className="block text-xs text-muted-foreground mt-1">
              Not available: git was not found. Install it and press “Check again” on the
              first step.
            </span>
          )}
        </span>
      </label>

      {fromSource && (
        <div className="flex flex-col gap-1.5 pl-6">
          <div className="text-sm font-medium">Branch</div>
          <Input
            value={branch}
            onChange={(event) => gear.set({ branch: event.target.value })}
            placeholder="the default branch"
            autoComplete="off"
            spellCheck={false}
            className="h-9"
          />
          <div className="text-xs text-muted-foreground">
            Left empty, the repository's own default branch is checked out — they do not
            all agree on what it is called.
          </div>
        </div>
      )}
    </div>
  );
};

/**
 * Where Rekuest comes from, asked where the consequence is visible.
 *
 * This used to be a step of its own ahead of the list, which asked about a service
 * before the list had said what any of them were. Taking Rekuest out of the hub is the
 * moment the question matters, so it is asked then: another one, or none.
 */
const RekuestChoiceBlock = ({
  choice,
  server,
  set,
}: {
  choice: RekuestChoice;
  server: string;
  set: (value: string) => void;
}) => {
  // Which of the two the user is on, rather than what the value happens to be: an
  // address that has not been typed yet is still an empty string, and the input has to
  // stay on screen while it is being filled in.
  const [wantsRemote, setWantsRemote] = useState(choice === "remote");

  if (choice === "local") {
    return (
      <p className="text-xs text-muted-foreground mt-4">
        Rekuest runs here, as part of this deployment. The usual choice. Remove it and
        you will be asked which Rekuest the other services should trust instead.
      </p>
    );
  }

  const remote = choice === "remote" || (wantsRemote && choice === "asking");

  return (
    <div className="mt-4 flex flex-col gap-3">
      <div className="text-sm">
        Rekuest will not run in this hub. Should its services trust one that runs
        somewhere else?
      </div>

      <div className="flex flex-col gap-2">
        <Card
          onClick={() => {
            setWantsRemote(true);
            if (!remote) set("");
          }}
          className={cn(
            "gap-1 py-3 px-4 cursor-pointer border transition-colors",
            remote ? "border-primary bg-primary/5" : "border-border"
          )}
        >
          <div className="font-medium text-sm">Use a Rekuest elsewhere</div>
          <div className="text-xs text-muted-foreground">
            The services here verify the provenance it signs, and no local Rekuest is
            started.
          </div>
          {remote && (
            <Input
              autoFocus
              value={server}
              onChange={(event) => set(event.target.value)}
              onClick={(event) => event.stopPropagation()}
              placeholder="rekuest.my-institute.org"
              autoComplete="off"
              spellCheck={false}
              className="h-9 mt-2"
            />
          )}
        </Card>

        <Card
          onClick={() => {
            setWantsRemote(false);
            set("none");
          }}
          className={cn(
            "gap-1 py-3 px-4 cursor-pointer border transition-colors",
            choice === "none" ? "border-primary bg-primary/5" : "border-border"
          )}
        >
          <div className="font-medium text-sm">Run without it</div>
          <div className="text-xs text-muted-foreground">
            No orchestration and no provenance authority. The other services still run,
            but nothing signs what happens on them.
          </div>
        </Card>
      </div>
    </div>
  );
};
