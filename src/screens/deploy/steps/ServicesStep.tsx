import { Boxes } from "lucide-react";
import { useEffect, useState } from "react";
import { useController, useWatch } from "react-hook-form";
import { Card } from "../../../components/ui/card";
import { ErrorDisplay } from "../../../components/Error";
import { cn } from "../../../utils";
import * as api from "../../../api";
import type { ServiceId, ServiceMeta } from "../../../api";
import { StepFrame } from "../../wizard/StepFrame";

/**
 * Which services this hub runs. Two rules are made visible here rather than silently
 * applied: Rekuest follows the "rekuest server" answer of the previous step, and
 * Lovekit is listed but has no published image, so ticking it would change nothing.
 *
 * The selection also decides what the coordination server is asked to adopt — every
 * service picked here becomes an instance in the hub manifest.
 */
export const ServicesStep = () => {
  const { field, fieldState } = useController<Record<string, ServiceId[]>>({
    name: "services",
  });
  const rekuestServer = (useWatch({ name: "rekuestServer" }) as string) ?? "local";
  const [services, setServices] = useState<ServiceMeta[]>([]);

  // The catalog — names, descriptions, which are pre-ticked — is published by the core,
  // so the wizard's list and the CLI's `--services` help cannot drift apart.
  useEffect(() => {
    api.serviceCatalog().then((catalog) => {
      setServices(catalog);
      if ((field.value ?? []).length === 0) {
        field.onChange(catalog.filter((s) => s.default).map((s) => s.id));
      }
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const rekuestIsLocal = rekuestServer.trim() === "local";

  const selected = new Set(field.value ?? []);

  const toggle = (id: ServiceId, locked: boolean) => {
    if (locked) return;
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    field.onChange(services.filter((s) => next.has(s.id)).map((s) => s.id));
  };

  return (
    <StepFrame
      icon={Boxes}
      title="Services"
      subtitle="What should this run?"
      lead="Each service is a separate container behind the gateway, and each one is registered with the coordination server under this hub."
    >
      <div className="grid grid-cols-1 @2xl:grid-cols-2 gap-2">
        {services.map((service) => {
          const isRekuest = service.id === "rekuest";
          const locked = isRekuest || !service.emitted;
          const checked = isRekuest
            ? rekuestIsLocal
            : service.emitted && selected.has(service.id);

          return (
            <Card
              key={service.id}
              onClick={() => toggle(service.id, locked)}
              className={cn(
                "gap-0 py-3 px-4 border transition-colors",
                locked ? "cursor-default" : "cursor-pointer",
                checked
                  ? "border-primary bg-primary/5"
                  : "border-border opacity-70"
              )}
            >
              <div className="flex flex-row items-start gap-3">
                <input
                  type="checkbox"
                  className="mt-1"
                  checked={checked}
                  disabled={locked}
                  readOnly
                />
                <div>
                  <div className="font-medium">{service.name}</div>
                  <div className="text-sm text-muted-foreground">
                    {service.description}
                  </div>
                  {isRekuest && (
                    <div className="text-xs mt-1 text-muted-foreground">
                      Follows the rekuest server you chose:{" "}
                      {rekuestIsLocal
                        ? "it runs here."
                        : `provided by ${rekuestServer}.`}
                    </div>
                  )}
                  {!service.emitted && (
                    <div className="text-xs mt-1 text-muted-foreground">
                      No image is published yet — enabling it would not change the
                      generated stack.
                    </div>
                  )}
                </div>
              </div>
            </Card>
          );
        })}
      </div>

      {fieldState.error && (
        <div className="max-w-xl mt-3">
          <ErrorDisplay name="services" />
        </div>
      )}
    </StepFrame>
  );
};
