import { open } from "@tauri-apps/plugin-shell";
import { BookOpen, Network, Waypoints } from "lucide-react";
import { useFormContext, useWatch } from "react-hook-form";

import { HubPicker, useRegisteredHubs } from "../../../components/engine/HubPicker";
import { Button } from "../../../components/ui/button";
import { Card } from "../../../components/ui/card";
import { DOCS } from "../../../docs";
import { cn } from "../../../utils";
import { StepFrame } from "../../wizard/StepFrame";

/**
 * How the engine's plugins reach a hub. Two answers, and they combine:
 *
 * - On the mesh, a Tailscale container runs beside the engine and plugins run inside its
 *   network, so they reach the hub the engine is bound to over the tailnet — from this
 *   machine or any other.
 * - Attached to a hub on this machine, the engine joins that hub's network, and plugins
 *   reach it directly inside Docker, as `gateway`.
 *
 * Neither: plugins reach hubs over the addresses those hubs advertise, like any client.
 */
export const EngineHubStep = () => {
  const { setValue } = useFormContext();
  const hub = useWatch({ name: "hub" }) as string | null;
  const mesh = !!useWatch({ name: "mesh" });
  const hubs = useRegisteredHubs();

  return (
    <StepFrame
      icon={Network}
      title="Reach"
      subtitle="How should its plugins reach your hub?"
      lead="Plugins this engine starts talk to a hub. They can reach it over the organization's mesh from anywhere, directly inside Docker when the hub runs on this machine, or both. You can change the hub later from the engine's page."
    >
      <div className="max-w-2xl flex flex-col gap-6">
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <div className="text-sm font-medium">Mesh</div>
            <Button size="sm" variant="ghost" onClick={() => open(DOCS.mesh)}>
              <BookOpen className="size-3.5" />
              How the mesh works
            </Button>
          </div>
          {[
            {
              value: true,
              title: "Join the mesh",
              body: "A small Tailscale container runs beside the engine, and every plugin runs inside its network, so plugins reach the hub the engine is bound to over the tailnet, from any machine. A mesh key is asked for when the engine is accepted; the engine is not created if none is granted.",
            },
            {
              value: false,
              title: "No mesh",
              body: "Plugins reach hubs at the addresses those hubs advertise, or directly inside Docker if you attach a hub below.",
            },
          ].map((option) => (
            <Card
              key={String(option.value)}
              onClick={() => setValue("mesh", option.value, { shouldDirty: true })}
              className={cn(
                "gap-0 py-3 px-4 cursor-pointer border transition-colors",
                mesh === option.value ? "border-primary bg-primary/5" : "border-border"
              )}
            >
              <div className="flex items-start gap-3">
                {option.value ? (
                  <Waypoints className="size-4 mt-0.5 shrink-0" />
                ) : (
                  <Network className="size-4 mt-0.5 shrink-0" />
                )}
                <div>
                  <div className="text-sm font-medium">{option.title}</div>
                  <div className="text-xs text-muted-foreground leading-relaxed mt-0.5">
                    {option.body}
                  </div>
                </div>
              </div>
            </Card>
          ))}
        </div>

        <div className="flex flex-col gap-2">
          <div className="text-sm font-medium">Hub on this machine</div>
          <p className="text-xs text-muted-foreground leading-relaxed">
            Attached, the engine joins the hub's network and plugins reach it directly
            inside Docker, which is the fastest path. The hub has to be running for the
            engine to start.
          </p>
          <HubPicker
            hubs={hubs}
            value={hub ?? null}
            onChange={(value) => setValue("hub", value, { shouldDirty: true })}
          />
          {hubs.length === 0 && (
            <p className="text-xs text-muted-foreground">
              There are no hubs on this machine yet.
            </p>
          )}
        </div>
      </div>
    </StepFrame>
  );
};
