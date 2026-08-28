import { Network } from "lucide-react";
import { useFormContext, useWatch } from "react-hook-form";
import { Card } from "../../../components/ui/card";
import { ErrorDisplay } from "../../../components/Error";
import { UIField } from "../../../components/FormInput";
import { cn } from "../../../utils";
import { StepField, StepFrame } from "../../wizard/StepFrame";

/**
 * Rekuest is the provenance authority every other service checks against, so it is
 * either part of this deployment or it lives somewhere else. The CLI encodes this as a
 * host string where `"local"` is special, and it overrides the service picker — the
 * next step shows that consequence instead of silently applying it.
 */
export const RekuestStep = () => {
  const { setValue } = useFormContext();
  const rekuestServer = (useWatch({ name: "rekuestServer" }) as string) ?? "local";
  const isLocal = rekuestServer.trim() === "local";

  return (
    <StepFrame
      icon={Network}
      title="Provenance"
      subtitle="Where does Rekuest run?"
      lead="Rekuest orchestrates tasks and signs the provenance of everything that happens on the platform. The other services verify those signatures, so they need to know which Rekuest to trust."
    >
      <div className="max-w-xl flex flex-col gap-2">
        <Card
          onClick={() =>
            setValue("rekuestServer", "local", { shouldValidate: true })
          }
          className={cn(
            "gap-1 py-3 px-4 cursor-pointer border transition-colors",
            isLocal ? "border-primary bg-primary/5" : "border-border"
          )}
        >
          <div className="font-medium">Run it here</div>
          <div className="text-sm text-muted-foreground">
            Rekuest becomes part of this deployment. The usual choice.
          </div>
        </Card>

        <Card
          onClick={() =>
            setValue("rekuestServer", isLocal ? "" : rekuestServer, {
              shouldValidate: true,
            })
          }
          className={cn(
            "gap-1 py-3 px-4 cursor-pointer border transition-colors",
            !isLocal ? "border-primary bg-primary/5" : "border-border"
          )}
        >
          <div className="font-medium">Use a remote Rekuest</div>
          <div className="text-sm text-muted-foreground">
            This deployment's services will trust a Rekuest that runs elsewhere, and no
            local Rekuest is started.
          </div>
        </Card>

        {!isLocal && (
          <div className="mt-3">
            <StepField label="Rekuest host">
              <UIField name="rekuestServer" autoComplete="off" spellCheck="false" />
              <ErrorDisplay name="rekuestServer" className="mt-1" />
            </StepField>
          </div>
        )}
      </div>
    </StepFrame>
  );
};
