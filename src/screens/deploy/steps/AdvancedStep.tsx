import { Cog, GitBranch, Package } from "lucide-react";
import { useEffect } from "react";
import { useFormContext, useWatch } from "react-hook-form";
import { useCommunication } from "../../../communication/communication-context";
import { ErrorDisplay } from "../../../components/Error";
import { UIField } from "../../../components/FormInput";
import { Card } from "../../../components/ui/card";
import { cn } from "../../../utils";
import { StepField, StepFrame } from "../../wizard/StepFrame";
import { HubForm } from "../hub-form";

/**
 * Where the services' code comes from.
 *
 * The ordinary hub runs the published images and touches nothing else. A *dev hub* checks
 * each enabled service's repository out into `mounts/<service>` and mounts it over the
 * image's workspace, so the containers run the source sitting on this machine — edit a
 * file, restart the service, see the change. The image is still what provides the
 * interpreter and the dependencies; only the code is replaced.
 *
 * It needs git, which is the one thing here Konstruktor cannot supply. Without it the
 * option is shown disabled rather than hidden: somebody who came looking for it deserves
 * to be told why it is not there.
 */
const SOURCES: {
  value: boolean;
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  body: string;
}[] = [
  {
    value: false,
    icon: Package,
    title: "Run the published images",
    body: "What almost every deployment wants. Each service runs exactly the image it ships as, and updates arrive by pulling a newer one.",
  },
  {
    value: true,
    icon: GitBranch,
    title: "Dev hub — run from a source checkout",
    body: "Clone each service's repository into mounts/ and mount it into its container. For working on the services themselves — the checkout is an ordinary clone, yours to edit, branch and push, and Konstruktor never writes over a file that is already there.",
  },
];

const SourcePicker = () => {
  const { setValue } = useFormContext();
  const values = useWatch() as HubForm;
  const { git } = useCommunication();

  const hasGit = git?.cli ?? false;
  const devHub = values.devHub ?? false;

  // A probe that comes back without git after the box was ticked — the user went back to
  // the Docker step and rechecked, say — must not leave an unbuildable answer standing.
  useEffect(() => {
    if (git && !git.cli && devHub) {
      setValue("devHub", false, { shouldValidate: true });
    }
  }, [git, devHub, setValue]);

  return (
    <div className="flex flex-col gap-2">
      {SOURCES.map((source) => {
        const disabled = source.value && !hasGit;
        const selected = devHub === source.value;
        const Icon = source.icon;
        return (
          <Card
            key={String(source.value)}
            onClick={() =>
              !disabled &&
              setValue("devHub", source.value, { shouldValidate: true })
            }
            className={cn(
              "gap-0 py-3 px-4 border transition-colors",
              disabled
                ? "border-border opacity-60 cursor-not-allowed"
                : "cursor-pointer",
              selected && !disabled ? "border-primary bg-primary/5" : "border-border"
            )}
          >
            <div className="flex items-start gap-3">
              <span
                className={cn(
                  "mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md border",
                  selected && !disabled
                    ? "border-primary text-primary"
                    : "border-border text-muted-foreground"
                )}
              >
                <Icon className="size-3.5" />
              </span>
              <div className="min-w-0">
                <div className="font-medium">{source.title}</div>
                <div className="text-sm text-muted-foreground mt-0.5">
                  {source.body}
                </div>
                {disabled && (
                  <div className="text-xs text-muted-foreground mt-1.5">
                    Not available: git was not found on this machine. Install it and
                    press “Check again” on the Docker step.
                  </div>
                )}
              </div>
            </div>
          </Card>
        );
      })}

      {devHub && hasGit && (
        <StepField
          label="Branch"
          hint="Checked out in every repository. Leave empty and each one uses its own default branch — they do not all agree on what it is called."
        >
          <UIField
            name="devBranch"
            placeholder="the default branch"
            autoComplete="off"
            spellCheck="false"
          />
          <ErrorDisplay name="devBranch" className="mt-1" />
        </StepField>
      )}
    </div>
  );
};

/**
 * The fields that are always written but rarely worth asking about: the deployment's
 * public domain, its description, and the Django superuser for the per-service admin
 * panels. Left blank, a generated 40-character password stands.
 */
export const AdvancedStep = () => {
  return (
    <StepFrame
      icon={Cog}
      title="Details"
      subtitle="Optional, and changeable later"
      lead="Everything here has a working default. Skip it unless you already know this deployment needs a public name, a specific admin account, or the services running from source."
    >
      <div className="max-w-xl flex flex-col gap-5">
        <StepField
          label="Domain"
          hint="The hostname this deployment will be reached under. Leave empty for a local deployment."
        >
          <UIField name="domain" placeholder="localhost" autoComplete="off" />
        </StepField>

        <StepField
          label="Description"
          hint="Shown to people connecting to this deployment."
        >
          <UIField name="globalDescription" autoComplete="off" />
        </StepField>

        <StepField
          label="Admin username"
          hint="The superuser for each service's own admin panel."
        >
          <UIField name="globalAdmin" autoComplete="off" spellCheck="false" />
          <ErrorDisplay name="globalAdmin" className="mt-1" />
        </StepField>

        <StepField
          label="Admin password"
          hint="Leave empty and a strong one is generated for you — you can read it on the dashboard afterwards."
        >
          <UIField name="globalAdminPassword" type="password" />
          <ErrorDisplay name="globalAdminPassword" className="mt-1" />
        </StepField>

        <StepField
          label="Where the code comes from"
          hint="A dev hub is for working on the services. If you are deploying one to use, leave this alone. Each container still reads the config Konstruktor generated, which is mounted over the repository's own config.yaml — the file on disk is left untouched."
        >
          <SourcePicker />
        </StepField>
      </div>
    </StepFrame>
  );
};
