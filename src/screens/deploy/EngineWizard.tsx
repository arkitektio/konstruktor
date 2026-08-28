import { ArrowLeft, ArrowRight, Container, FolderOpen, Rocket, Server } from "lucide-react";
import { useState } from "react";
import { useWatch } from "react-hook-form";
import { Link, useNavigate } from "react-router-dom";
import { z } from "zod";

import * as api from "../../api";
import type { CreateEvent, EngineAnswers } from "../../api";
import { Button } from "../../components/ui/button";
import { WizardPage } from "../../layout/WizardPage";
import { useRegistry } from "../../registry/registry-context";
import { useSettings } from "../../settings/settings-context";
import { Wizard, WizardRenderProps, WizardStep } from "../wizard/Wizard";
import {
  CreateState,
  emptyCreateState,
  InstallProgress,
  reduceCreate,
} from "./InstallProgress";
import { coordinationServerSchema } from "./hub-form";
import { CoordinationStep } from "./steps/CoordinationStep";
import { DockerStep } from "./steps/DockerStep";
import { FolderStep } from "./steps/FolderStep";
import { SummaryStep } from "./steps/SummaryStep";

/**
 * Creating a plugin engine.
 *
 * Four questions against a hub's nine, and that is the point of it being its own path:
 * an engine is one container with the Docker socket, so there are no services to pick,
 * no ports to bind, no addresses to advertise and no mesh to join. The steps it does
 * share — Docker, the folder, the coordination server — are the same components the hub
 * wizard uses, so the two cannot drift.
 */

/** The engine wizard's form. A subset of the hub's, and deliberately not the same type. */
export type EngineForm = {
  dockerOk: boolean;
  path: string;
  name: string;
  folderOk: boolean;
  coordServer: string;
  identifier: string;
  description: string;
};

const toAnswers = (values: EngineForm): EngineAnswers => ({
  dir: values.path,
  name: values.name.trim(),
  coord_server: values.coordServer.trim(),
  identifier: values.identifier.trim(),
  description: values.description?.trim() || null,
  // Written first, started from the dashboard — the same rule the hub wizard follows, so
  // the first `up` happens where its output and the container list already are.
  start: false,
});

const EngineSummary = () => {
  const values = useWatch() as EngineForm;

  return (
    <SummaryStep
      title="Ready"
      subtitle="Nothing has been written yet — you will be asked to accept the engine first"
      rows={[
        { label: "Folder", value: values.path ?? "" },
        { label: "Name", value: values.name ?? "" },
        { label: "Coordination server", value: values.coordServer ?? "" },
        { label: "Engine identifier", value: values.identifier ?? "" },
        { label: "Runs", value: "jhnnsrs/deployer:next, with this machine's Docker socket" },
      ]}
      files={["docker-compose.yaml", "configs/deployer.yaml"]}
    />
  );
};

export const EngineWizard = () => {
  const navigate = useNavigate();
  const { refresh } = useRegistry();
  const { settings, setSettings } = useSettings();

  const [creating, setCreating] = useState<CreateState>(emptyCreateState);

  const initialValues: EngineForm = {
    dockerOk: false,
    path: "",
    name: "",
    folderOk: false,
    coordServer: settings.coordinationServer,
    identifier: "",
    description: "",
  };

  const steps: WizardStep[] = [
    {
      component: DockerStep,
      meta: { label: "Docker", title: "Docker", icon: Container },
      validationSchema: z.looseObject({
        dockerOk: z
          .boolean()
          .refine((ok) => ok, "Docker has to be ready before an engine can be created"),
      }),
    },
    {
      component: () => <FolderStep kind={{ label: "Engine" }} />,
      meta: { label: "Folder", title: "Folder", icon: FolderOpen },
      validationSchema: z.looseObject({
        path: z.string().min(1, "Choose a folder for this engine"),
        name: z
          .string()
          .trim()
          .min(2, "At least two characters")
          .max(40, "At most 40 characters"),
        folderOk: z.boolean().refine((ok) => ok, "This folder cannot be used, see above"),
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
      component: EngineSummary,
      meta: { label: "Review", title: "Review", icon: Rocket },
    },
  ];

  const handleSubmit = async (values: EngineForm) => {
    setCreating({ ...emptyCreateState, running: true });

    const onEvent = (event: CreateEvent) =>
      setCreating((previous) => reduceCreate(previous, event));

    try {
      await api.createEngine(toAnswers(values), onEvent);
    } catch (error) {
      setCreating((previous) => ({
        ...previous,
        running: false,
        error: typeof error === "string" ? error : String(error),
      }));
      return;
    }

    setCreating((previous) => ({ ...previous, running: false, done: true }));

    const server = values.coordServer.trim();
    const known = settings.knownCoordinationServers ?? [];
    await setSettings({
      ...settings,
      coordinationServer: server,
      knownCoordinationServers: known.includes(server) ? known : [...known, server],
    });

    await refresh();
    const created = (await api.listDeployments()).find((d) => d.path === values.path);
    if (created) navigate(`/dashboard/${created.id}`);
  };

  return (
    <>
      <InstallProgress
        open={creating.running || creating.done || creating.error !== null}
        state={creating}
        onClose={() => setCreating(emptyCreateState)}
        kind="engine"
      />
      <Wizard<EngineForm>
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
            title="New plugin engine"
            rail={rail}
            position={position}
            total={total}
            onJump={goBackTo}
            stepKey={currentStepIndex}
            buttons={
              <>
                {(isValid || isSubmitting) && (
                  <Button disabled={isNextDisabled} onClick={handleNext}>
                    {isSubmitting
                      ? "Creating…"
                      : isLastStep
                        ? "Create the engine"
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
                    <Link to="/new">Cancel</Link>
                  </Button>
                ) : (
                  <Button variant="outline" disabled={isPrevDisabled} onClick={handlePrev}>
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
