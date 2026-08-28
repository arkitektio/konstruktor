import { Cog } from "lucide-react";
import { ErrorDisplay } from "../../../components/Error";
import { UIField } from "../../../components/FormInput";
import { StepField, StepFrame } from "../../wizard/StepFrame";

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
      lead="Everything here has a working default. Skip it unless you already know this deployment needs a public name or a specific admin account."
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
      </div>
    </StepFrame>
  );
};
