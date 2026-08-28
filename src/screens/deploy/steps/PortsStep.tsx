import { Plug } from "lucide-react";
import { useFormContext } from "react-hook-form";
import { ErrorDisplay } from "../../../components/Error";
import { UIField } from "../../../components/FormInput";
import { AdvancedFields, StepField, StepFrame } from "../../wizard/StepFrame";

/**
 * The ports Caddy publishes on the host. Everything else is routed behind them by
 * path prefix, so these two numbers are the whole external surface of a deployment.
 */
export const PortsStep = () => {
  const { register } = useFormContext();

  return (
    <StepFrame
      icon={Plug}
      title="Ports"
      subtitle="How it is reached"
      lead={
        <>
          The gateway publishes these ports on this machine, and routes every service
          behind them — <code>/rekuest</code>, <code>/mikro</code> and so on. The
          defaults are high ports nothing else is likely to hold; change them if this
          machine already uses them, or set 80 and 443 to serve on the usual ones.
        </>
      }
    >
      <div className="max-w-md flex flex-col gap-5">
        <StepField label="HTTP port">
          <UIField name="httpPort" type="number" inputMode="numeric" />
          <ErrorDisplay name="httpPort" className="mt-1" />
        </StepField>
        <AdvancedFields fields={["httpsPort"]}>
          <StepField
            label="HTTPS port"
            hint="Reserved on the host. The generated gateway serves plain HTTP for now."
          >
            <UIField name="httpsPort" type="number" inputMode="numeric" />
            <ErrorDisplay name="httpsPort" className="mt-1" />
          </StepField>
          <label className="flex flex-row items-center gap-2 text-sm">
            <input type="checkbox" {...register("ssl")} />
            Advertise this deployment as HTTPS to the coordination server
          </label>
        </AdvancedFields>
      </div>
    </StepFrame>
  );
};
