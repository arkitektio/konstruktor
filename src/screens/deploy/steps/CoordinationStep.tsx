import { Server } from "lucide-react";
import { useEffect, useRef } from "react";
import { useController, useFormState } from "react-hook-form";
import { CoordinationPicker } from "../../../coordination/CoordinationPicker";
import { ErrorDisplay } from "../../../components/Error";
import { UIField } from "../../../components/FormInput";
import { Alert } from "../../../components/ui/alert";
import { useSettings } from "../../../settings/settings-context";
import { AdvancedFields, StepField, StepFrame } from "../../wizard/StepFrame";

/**
 * Who vouches for this hub, and what it is called there.
 *
 * A hub manages no accounts of its own: users, organizations and permissions all live
 * on the coordination server, and the hub is a member of one of its organizations. The
 * identifier is how it is known inside that organization, so it has to be unique there
 * — it is not a display name that can be changed later without consequence.
 */
export const CoordinationStep = () => {
  const { field } = useController({ name: "coordServer" });
  const { dirtyFields } = useFormState({ name: "coordServer" });
  const { settings } = useSettings();

  // The wizard's defaults are taken when the form mounts, which can be before the stored
  // settings have been read back — so the remembered server would show up as an offered
  // card that is not the selected one. Adopt it once, and never over an answer given.
  const adopted = useRef(false);
  useEffect(() => {
    if (adopted.current || dirtyFields.coordServer) return;
    if (!settings.coordinationServer) return;
    adopted.current = true;
    if (settings.coordinationServer === field.value) return;
    field.onChange(settings.coordinationServer);
  }, [settings.coordinationServer, dirtyFields.coordServer, field]);

  return (
    <StepFrame
      icon={Server}
      title="Coordination"
      subtitle="Who does this hub answer to?"
      lead="Accounts, organizations and permissions live on a coordination server. This hub runs the services and trusts that server to say who anyone is. You will be asked to accept it there, in a browser, before anything is written to disk."
    >
      <div className="flex flex-col gap-6 max-w-xl">
        <StepField label="Coordination server">
          <CoordinationPicker value={field.value ?? ""} onChange={field.onChange} />
          <ErrorDisplay name="coordServer" className="mt-1" />
        </StepField>

        <StepField
          label="Hub identifier"
          hint="How this hub is known inside the organization that accepts it. Unique there."
        >
          <UIField name="identifier" autoComplete="off" spellCheck="false" />
          <ErrorDisplay name="identifier" className="mt-1" />
        </StepField>

        <AdvancedFields>
          <StepField
            label="Description"
            hint="Shown to whoever is asked to accept the hub. Optional."
          >
            <UIField name="description" autoComplete="off" />
          </StepField>
        </AdvancedFields>

        <Alert className="text-xs text-muted-foreground">
          Nothing is created on the coordination server yet — that happens at the end,
          once you have chosen what this hub will run.
        </Alert>
      </div>
    </StepFrame>
  );
};
