import {
  ArrowLeft,
  ArrowRight,
  Boxes,
  Container,
  Database,
  FolderOpen,
  Globe,
  Plug,
  Rocket,
  Server,
  Waypoints,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useWatch } from "react-hook-form";
import { Link, useNavigate } from "react-router-dom";
import { z } from "zod";
import { Button } from "../../components/ui/button";
import { WizardPage } from "../../layout/WizardPage";
import { Wizard, WizardRenderProps, WizardStep } from "../wizard/Wizard";
import { DockerStep } from "./steps/DockerStep";
import { MeshStep } from "./steps/MeshStep";
import { FolderStep } from "./steps/FolderStep";
import { CoordinationStep } from "./steps/CoordinationStep";
import { ServicesStep } from "./steps/ServicesStep";
import { PortsStep } from "./steps/PortsStep";
import { StorageStep } from "./steps/StorageStep";
import { HostsStep } from "./steps/HostsStep";
import { SummaryStep } from "./steps/SummaryStep";
import {
  InstallProgress,
  CreateState,
  emptyCreateState,
  reduceCreate,
} from "./InstallProgress";
import * as api from "../../api";
import type {
  AdvertisedHost,
  CreateEvent,
  HubAnswers,
  ServiceId,
} from "../../api";
import { useRegistry } from "../../registry/registry-context";
import { useSettings } from "../../settings/settings-context";
import {
  HubForm,
  ServiceOverride,
  baseUrl,
  coordinationServerSchema,
  serviceAnswer,
} from "./hub-form";

/**
 * Creating a hub, end to end.
 *
 * The order is not cosmetic. The manifest sent to the coordination server declares every
 * service instance and the addresses it can be reached at, so the folder, the services,
 * the ports and the addresses all have to be answered before the hub can be authorized —
 * and the hub has to be authorized before anything is written, because the identity it
 * comes back with is what the generated service configs trust.
 */

const port = z.coerce
  .number()
  .int("Must be a whole number")
  .min(1, "Must be above 0")
  .max(65535, "Must be below 65536");

/** The last screen: what is about to be created, and what will land in the folder. */
const HubSummary = () => {
  const values = useWatch() as HubForm;
  const [files, setFiles] = useState<string[]>([]);

  // The file list comes from the generator itself rather than a list kept in step with
  // it by hand — this is a preview, so the throwaway config it builds costs nothing.
  useEffect(() => {
    let cancelled = false;
    api
      .previewHubFiles(toAnswers(values))
      .then((names) => !cancelled && setFiles(names.sort()))
      .catch(() => !cancelled && setFiles([]));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
    // `serviceOptions` is in here because a service running from source adds bind mounts
    // to the compose file, which is one of the files this list previews.
  }, [
    values.services,
    values.rekuestServer,
    values.httpPort,
    values.httpsPort,
    values.ssl,
    values.serviceOptions,
  ]);

  return (
    <SummaryStep
      title="Ready"
      subtitle="Nothing has been written yet"
      rows={[
        { label: "Folder", value: values.path ?? "" },
        { label: "Name", value: values.name ?? "" },
        { label: "Coordination server", value: values.coordServer ?? "" },
        { label: "Hub identifier", value: values.identifier ?? "" },
        {
          label: "Rekuest",
          value: rekuestSummary(values.rekuestServer ?? "local"),
        },
        { label: "Services", value: (values.services ?? []).join(", ") || "none" },
        { label: "Ports", value: `${values.httpPort} / ${values.httpsPort}` },
        { label: "Storage", value: storageSummary(values.storage) },
        {
          label: "Advertised at",
          value: advertisedAt(values.hosts ?? []),
        },
        {
          label: "From source",
          value: fromSourceSummary(values.serviceOptions ?? {}),
        },
        {
          label: "Language models",
          value: ollamaSummary(values.serviceOptions ?? {}, values.services ?? []),
        },
        {
          label: "Debug mode",
          value: debugSummary(values.serviceOptions ?? {}),
        },
        {
          label: "Mesh",
          value:
            values.meshMode === "none"
              ? ""
              : values.meshMode === "coordination"
                ? "asks the coordination server for a key"
                : "uses the key you supplied",
        },
      ]}
      files={["hub_config.yaml", "hub_credentials.json", ...files]}
    />
  );
};

/**
 * The Rekuest row, which has three answers rather than two: it runs here, it runs at an
 * address the services trust, or there is none — and "none" is a word the core reads,
 * not something to print at somebody as if it were a hostname.
 */
const rekuestSummary = (server: string): string => {
  const value = server.trim();
  if (value === "local" || value === "") return "runs here";
  if (value === "none") return "none — nothing signs provenance";
  return value;
};

/**
 * Where Alpaka's models will come from, for the review step.
 *
 * Worth a row of its own because the answer decides whether the deployment gains a
 * container, and because saying nothing is a real outcome: Alpaka runs and answers
 * nothing, which is better seen before the hub is created than after.
 */
const ollamaSummary = (
  options: Partial<Record<ServiceId, ServiceOverride>>,
  services: ServiceId[]
): string => {
  if (!services.includes("alpaka")) return "";
  const asked = options.alpaka;
  if (asked?.ollama === "local") return "Ollama, in this deployment";
  if (asked?.ollama === "remote" && asked.ollamaUrl.trim())
    return asked.ollamaUrl.trim();
  return "none yet — Alpaka will have no provider";
};

/**
 * The services shipping with Django's debug mode on.
 *
 * A row rather than a footnote: debug shows internals to anyone who can reach the
 * service, and leaving it on by accident is the kind of thing a review step exists for.
 */
const debugSummary = (
  options: Partial<Record<ServiceId, ServiceOverride>>
): string => {
  const on = Object.entries(options)
    .filter(([, asked]) => asked?.debug)
    .map(([id]) => id);
  return on.length === 0 ? "" : `on for ${on.join(", ")}`;
};

/**
 * Which services will run from a checkout rather than their published image, for the
 * review step. Empty when none do, which is the ordinary hub and needs no row.
 */
const fromSourceSummary = (
  options: Partial<Record<string, { fromSource: boolean; branch: string }>>
): string => {
  const named = Object.entries(options)
    .filter(([, asked]) => asked?.fromSource)
    .map(([id, asked]) =>
      asked?.branch?.trim() ? `${id} (${asked.branch.trim()})` : id
    );
  return named.length === 0 ? "" : named.join(", ");
};

/**
 * The addresses, for the review step.
 *
 * "Local only" is a legitimate answer now that loopback is offered, and a bare
 * `127.0.0.1` in a summary row reads like something went wrong rather than like a choice.
 */
const advertisedAt = (hosts: AdvertisedHost[]): string => {
  if (hosts.length === 0) return "nothing";
  if (hosts.every((host) => host.kind === "loopback")) return "only this machine";
  return hosts.map((host) => host.host).join(", ");
};

/**
 * Where the data goes, for the review step. The slow choice is named as such one last
 * time — the step's warning is behind a Previous button by now.
 */
const storageSummary = (storage: HubForm["storage"]): string =>
  storage === "deployment-folder"
    ? "folders inside the deployment (slow on Docker Desktop)"
    : "Docker volumes";

/** The form, as the core wants it: flat, snake_case, and already trimmed. */
const toAnswers = (values: HubForm): HubAnswers => ({
  dir: values.path,
  name: values.name.trim(),
  coord_server: values.coordServer.trim(),
  identifier: values.identifier.trim(),
  description: values.description?.trim() || null,
  rekuest_server: values.rekuestServer.trim(),
  services: values.services,
  http_port: Number(values.httpPort),
  https_port: Number(values.httpsPort),
  ssl: values.ssl,
  domain: values.domain?.trim() || null,
  // Nobody is asked for these any more: the wizard has no Details step, and the account
  // that matters is made per service on the dashboard, against a container that is up.
  // The generated profile still needs a name and a password for the one it seeds, and an
  // empty password is what tells the core to generate a strong one.
  global_admin: values.globalAdmin.trim(),
  global_admin_password: values.globalAdminPassword || null,
  global_description: values.globalDescription?.trim() || null,
  hosts: values.hosts ?? [],
  // Nothing is listening while the wizard runs, so no probe can have confirmed
  // anything. Aliases go out unmarked and the dashboard can check later.
  reachable_hosts: [],
  mesh_mode: values.meshMode,
  mesh_auth_key: values.meshAuthKey || null,
  mesh_coord_url: values.meshCoordUrl || null,
  // The wizard writes the deployment and stops there. Starting it is the dashboard's
  // job, so the first `up` happens where its output and the container list already are.
  start: false,
  // The wizard never asks for a whole dev hub any more: it asks per service, and the
  // core takes the union of the two.
  dev_hub: false,
  dev_branch: null,
  // Only the services somebody actually changed something on. The filter used to be
  // `fromSource` alone, which would silently drop every other answer the gear collects.
  service_options: Object.fromEntries(
    Object.entries(values.serviceOptions ?? {})
      .map(([id, asked]) => [id, asked && serviceAnswer(asked)] as const)
      .filter(([, answer]) => answer !== undefined)
  ),
  storage: values.storage ?? "docker-volumes",
});

export const HubWizard = () => {
  const kind = { label: "Hub" };
  const navigate = useNavigate();
  const { refresh } = useRegistry();
  const { settings, setSettings } = useSettings();

  const [creating, setCreating] = useState<CreateState>(emptyCreateState);

  const initialValues: HubForm = {
    dockerOk: false,
    path: "",
    name: "",
    folderOk: false,
    coordServer: settings.coordinationServer,
    identifier: "",
    description: "",
    rekuestServer: "local",
    services: [],
    httpPort: 7080,
    httpsPort: 7443,
    ssl: false,
    domain: "",
    globalDescription: "",
    globalAdmin: "admin",
    globalAdminPassword: "",
    hosts: [],
    meshMode: "none",
    meshAuthKey: "",
    meshCoordUrl: "",
    serviceOptions: {},
    storage: "docker-volumes",
  };

  const steps: WizardStep[] = useMemo(
    () => [
      {
        component: DockerStep,
        meta: {
          label: "Docker",
          title: "Docker",
          icon: Container,
        },
        // The check is not advisory. Everything after this step describes a stack that
        // Docker Compose is the only thing that will ever run.
        validationSchema: z.looseObject({
          dockerOk: z
            .boolean()
            .refine((ok) => ok, "Docker has to be ready before a hub can be created"),
        }),
      },
      {
        component: () => <FolderStep kind={kind} />,
        meta: { label: "Folder", title: "Folder", icon: FolderOpen },
        validationSchema: z.looseObject({
          path: z.string().min(1, "Choose a folder for this deployment"),
          name: z
            .string()
            .trim()
            .min(2, "At least two characters")
            .max(40, "At most 40 characters"),
          folderOk: z
            .boolean()
            .refine((ok) => ok, "This folder cannot be used, see above"),
        }),
      },
      {
        component: CoordinationStep,
        meta: { label: "Coordination", title: "Coordination", icon: Server },
        validationSchema: z.looseObject({
          coordServer: coordinationServerSchema,
          identifier: z
            .string()
            .trim()
            .min(2, "At least two characters")
            .max(60, "At most 60 characters")
            .regex(
              /^[a-zA-Z0-9][a-zA-Z0-9._-]*$/,
              "Letters, digits, dots, dashes and underscores"
            ),
        }),
      },
      {
        // Provenance is no longer a step of its own: it asked where Rekuest runs before
        // the list had said what Rekuest was. The services step asks it at the moment
        // Rekuest is taken out of the hub, which is when the answer matters.
        component: ServicesStep,
        meta: { label: "Services", title: "Services", icon: Boxes },
        validationSchema: z.looseObject({
          // An empty selection would produce a stack with nothing but infrastructure
          // in it, which generation would happily accept.
          services: z.array(z.string()).min(1, "Pick at least one service"),
          // "local" runs it here, "none" is a hub with no provenance authority at all,
          // anything else is the host of a Rekuest elsewhere. Empty is the state the
          // question passes through before it is answered, and it does not validate.
          rekuestServer: z
            .string()
            .trim()
            .refine(
              (value) => value.length > 0,
              "Say which Rekuest this hub should trust"
            )
            .refine((value) => !/\s/.test(value), "A hostname cannot contain spaces"),
          // A branch name is git's to validate; only the shapes it can never accept are
          // rejected here, so a typo is caught before the clone is attempted.
          serviceOptions: z
            .record(
              z.string(),
              z.looseObject({ fromSource: z.boolean(), branch: z.string() })
            )
            .refine(
              (map) =>
                // Only what will actually be cloned. A bad branch left behind on a
                // service that no longer runs from source would hold the step invalid
                // with no field on screen to fix it in.
                Object.values(map).every((asked) => {
                  const branch = asked.branch.trim();
                  return !asked.fromSource || branch === "" ||
                    !/[\s~^:?*\[\\]/.test(branch);
                }),
              "That is not a valid branch name"
            )
            // Picking "use one that already exists" and leaving the box empty would send
            // no provider at all — the answer is dropped, and Alpaka comes up with
            // nothing to talk to. Hold the step instead of losing it quietly.
            .refine(
              (map) =>
                Object.values(map).every(
                  (asked) =>
                    asked.ollama !== "remote" ||
                    String(asked.ollamaUrl ?? "").trim() !== ""
                ),
              "Give the address of the Ollama to use, or choose to run one here"
            ),
        }),
      },
      {
        component: PortsStep,
        meta: { label: "Ports", title: "Ports", icon: Plug },
        validationSchema: z
          .looseObject({ httpPort: port, httpsPort: port })
          .refine((v) => v.httpPort !== v.httpsPort, {
            error: "The two ports must differ",
            path: ["httpsPort"],
          }),
      },
      {
        component: StorageStep,
        meta: { label: "Storage", title: "Storage", icon: Database },
        validationSchema: z.looseObject({
          storage: z.enum(["docker-volumes", "deployment-folder"]),
        }),
      },
      {
        component: HostsStep,
        meta: { label: "Addresses", title: "Addresses", icon: Globe },
        validationSchema: z.looseObject({
          hosts: z
            .array(z.looseObject({ host: z.string() }))
            .min(1, "Pick at least one address — without one, nothing can find this hub"),
        }),
      },
      {
        component: MeshStep,
        meta: { label: "Mesh", title: "Mesh", icon: Waypoints },
        validationSchema: z
          .looseObject({
            meshMode: z.enum(["none", "coordination", "manual"]),
            meshAuthKey: z.string(),
            meshCoordUrl: z.string(),
          })
          .refine(
            (v) => v.meshMode !== "manual" || v.meshAuthKey.trim().length > 0,
            { error: "Paste the mesh key, or pick another option", path: ["meshAuthKey"] }
          )
          .refine(
            (v) => {
              const url = v.meshCoordUrl.trim();
              if (v.meshMode !== "manual" || url === "") return true;
              try {
                return new URL(baseUrl(url)).hostname.length > 0;
              } catch {
                return false;
              }
            },
            { error: "That does not look like a server address", path: ["meshCoordUrl"] }
          ),
      },
      {
        component: HubSummary,
        meta: { label: "Review", title: "Review", icon: Rocket },
      },
    ],
    [kind]
  );

  /**
   * One call builds the profile, authorizes it and writes the folder — it does not start
   * the stack. Progress, including the device code somebody has to accept, comes back
   * through a channel and is rendered by {@link InstallProgress}; when it is done we go
   * to the hub's dashboard, where Start is.
   */
  const handleSubmit = async (values: HubForm) => {
    setCreating({ ...emptyCreateState, running: true });

    const onEvent = (event: CreateEvent) =>
      setCreating((previous) => reduceCreate(previous, event));

    try {
      await api.createHub(toAnswers(values), onEvent);
    } catch (error) {
      setCreating((previous) => ({
        ...previous,
        running: false,
        error: typeof error === "string" ? error : String(error),
      }));
      return;
    }

    setCreating((previous) => ({ ...previous, running: false, done: true }));

    // Offer this server first next time, and keep it in the picker's list — the second
    // hub on a machine almost always answers to the same one as the first.
    const server = values.coordServer.trim();
    const known = settings.knownCoordinationServers ?? [];
    await setSettings({
      ...settings,
      coordinationServer: server,
      knownCoordinationServers: known.some(
        (entry) => baseUrl(entry).toLowerCase() === baseUrl(server).toLowerCase()
      )
        ? known
        : [...known, server],
    });

    // The core registered it; re-read so the new hub is there to navigate to.
    await refresh();
    const created = (await api.listDeployments()).find(
      (d) => d.path === values.path
    );
    if (created) navigate(`/dashboard/${created.id}`);
  };

  return (
    <>
      <InstallProgress
        open={creating.running || creating.done || creating.error !== null}
        state={creating}
        onClose={() => setCreating(emptyCreateState)}
      />
      <Wizard<HubForm>
        initialValues={initialValues}
        steps={steps}
        onSubmit={handleSubmit as any}
      >
        {({
          currentStepIndex,
          rail,
          position,
          total,
          renderComponent,
          handlePrev,
          handleNext,
          goBackTo,
          isSubmitting,
          isValid,
          isNextDisabled,
          isPrevDisabled,
          isLastStep,
        }: WizardRenderProps) => (
          <WizardPage
            title={`New ${kind.label.toLowerCase()}`}
            rail={rail}
            position={position}
            total={total}
            onJump={goBackTo}
            stepKey={currentStepIndex}
            buttons={
              <>
                {/*
                  Absent, not greyed out, until the step is answered. A disabled button
                  invites clicking at it to find out why; the step says what it still
                  wants, and the button turns up when it has it.
                */}
                {(isValid || isSubmitting) && (
                  <Button disabled={isNextDisabled} onClick={handleNext}>
                    {isSubmitting
                      ? "Creating…"
                      : isLastStep
                        ? "Create the hub"
                        : "Next"}
                    {isLastStep ? (
                      <Rocket className="size-3.5" />
                    ) : (
                      <ArrowRight className="size-3.5" />
                    )}
                  </Button>
                )}
                {currentStepIndex === 0 ? (
                  <Button variant="ghost" asChild>
                    <Link to="/">Cancel</Link>
                  </Button>
                ) : (
                  <Button
                    variant="outline"
                    disabled={isPrevDisabled}
                    onClick={handlePrev}
                  >
                    <ArrowLeft className="size-3.5" />
                    Previous
                  </Button>
                )}
              </>
            }
          >
            {renderComponent()}
          </WizardPage>
        )}
      </Wizard>
    </>
  );
};
