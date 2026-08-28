import {
  ArrowLeft,
  ArrowRight,
  Boxes,
  Cog,
  Container,
  FolderOpen,
  Globe,
  Network,
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
import { RekuestStep } from "./steps/RekuestStep";
import { ServicesStep } from "./steps/ServicesStep";
import { PortsStep } from "./steps/PortsStep";
import { HostsStep } from "./steps/HostsStep";
import { AdvancedStep } from "./steps/AdvancedStep";
import { SummaryStep } from "./steps/SummaryStep";
import { InstallProgress, CreateState, emptyCreateState } from "./InstallProgress";
import * as api from "../../api";
import type { CreateEvent, HubAnswers, ServiceMeta } from "../../api";
import { useRegistry } from "../../registry/registry-context";
import { useSettings } from "../../settings/settings-context";
import { HubForm, baseUrl, coordinationServerSchema } from "./hub-form";

/**
 * Creating a hub, end to end.
 *
 * The order is not cosmetic. The manifest sent to the coordination server declares every
 * service instance and the addresses it can be reached at, so the folder, the services,
 * the ports and the addresses all have to be answered before the hub can be authorized —
 * and the hub has to be authorized before anything is written, because the identity it
 * comes back with is what the generated service configs trust.
 */

const hostname = z
  .string()
  .trim()
  .min(1, "Required")
  .refine((value) => !/\s/.test(value), "A hostname cannot contain spaces");

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
  }, [values.services, values.rekuestServer, values.httpPort, values.httpsPort, values.ssl]);

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
          value:
            (values.rekuestServer ?? "local").trim() === "local"
              ? "runs here"
              : values.rekuestServer,
        },
        { label: "Services", value: (values.services ?? []).join(", ") || "none" },
        { label: "Ports", value: `${values.httpPort} / ${values.httpsPort}` },
        {
          label: "Advertised at",
          value: (values.hosts ?? []).map((h) => h.host).join(", "),
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
  global_admin: values.globalAdmin.trim(),
  global_admin_password: values.globalAdminPassword || null,
  global_description: values.globalDescription?.trim() || null,
  hosts: values.hosts ?? [],
  mesh_mode: values.meshMode,
  mesh_auth_key: values.meshAuthKey || null,
  mesh_coord_url: values.meshCoordUrl || null,
  start: true,
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
    httpPort: 80,
    httpsPort: 443,
    ssl: false,
    domain: "",
    globalDescription: "",
    globalAdmin: "admin",
    globalAdminPassword: "",
    hosts: [],
    meshMode: "none",
    meshAuthKey: "",
    meshCoordUrl: "",
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
        component: RekuestStep,
        meta: { label: "Rekuest", title: "Rekuest", icon: Network },
        validationSchema: z.looseObject({ rekuestServer: hostname }),
      },
      {
        component: ServicesStep,
        meta: { label: "Services", title: "Services", icon: Boxes },
        validationSchema: z.looseObject({
          // An empty selection would produce a stack with nothing but infrastructure
          // in it, which generation would happily accept.
          services: z.array(z.string()).min(1, "Pick at least one service"),
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
        component: HostsStep,
        meta: { label: "Addresses", title: "Addresses", icon: Globe },
        validationSchema: z.looseObject({
          hosts: z
            .array(z.looseObject({ host: z.string() }))
            .min(1, "Pick at least one address, or nobody can reach this hub"),
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
        component: AdvancedStep,
        meta: { label: "Advanced", title: "Advanced", icon: Cog },
        validationSchema: z.looseObject({
          globalAdmin: z.string().trim().min(1, "Required"),
          globalAdminPassword: z
            .string()
            .refine(
              (value) => value === "" || value.length >= 8,
              "At least 8 characters, or leave empty to generate one"
            ),
        }),
      },
      {
        component: HubSummary,
        meta: { label: "Review", title: "Review", icon: Rocket },
      },
    ],
    [kind]
  );

  /**
   * One call does the lot: build the profile, authorize it, write the folder, start the
   * stack. Progress — including the device code somebody has to accept — comes back
   * through a channel and is rendered by {@link InstallProgress}.
   */
  const handleSubmit = async (values: HubForm) => {
    setCreating({ ...emptyCreateState, running: true });

    const onEvent = (event: CreateEvent) =>
      setCreating((previous) => ({
        ...previous,
        event,
        logs:
          event.event === "log"
            ? [...previous.logs, event.line]
            : event.event === "writing"
              ? [...previous.logs, `wrote ${event.file}`]
              : previous.logs,
      }));

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
