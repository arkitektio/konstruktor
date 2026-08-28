import { KeyRound, Network, Waypoints } from "lucide-react";
import { useFormContext, useWatch } from "react-hook-form";
import { ErrorDisplay } from "../../../components/Error";
import { UIField } from "../../../components/FormInput";
import { Alert } from "../../../components/ui/alert";
import { Card } from "../../../components/ui/card";

import { cn } from "../../../utils";
import { AdvancedFields, StepField, StepFrame } from "../../wizard/StepFrame";
import type { MeshMode } from "../../../api";
import { HubForm } from "../hub-form";

/**
 * A preview of the name this hub will take on the tailnet.
 *
 * The fold that actually decides it lives in `konstruktor-core`; this only has to agree
 * closely enough to show the user what to expect, and never feeds anything.
 */
const previewHostname = (identifier: string): string =>
  identifier
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 63);

/**
 * Whether this hub joins a mesh, and where its key comes from.
 *
 * A hub advertised only at LAN addresses is only reachable from that LAN. The mesh is
 * the way out: a Tailscale sidecar joins the hub to the organization's tailnet and the
 * gateway is published inside that container's network namespace, so the hub is on the
 * tailnet under a name of its own.
 *
 * Joining is not the same as being advertised, and the step says so. The manifest sent
 * at authorization carries the addresses chosen on `HostsStep`, and the tailnet address
 * does not exist until the hub has actually joined — so the second half is a trip
 * through `ConnectScreen` once the stack is up. Guessing the address here would put an
 * alias in the manifest that may never resolve.
 *
 * The key is a single-use pre-authorized key, and there are two honest ways to get one.
 * The coordination server mints one as a side effect of the authorization this wizard
 * already performs — that is what `request_auth_key` on the hub manifest asks for, and
 * it costs the user nothing extra. Failing that, a key from a tailnet of their own can
 * be pasted, together with the control server it belongs to.
 */

const OPTIONS: {
  value: MeshMode;
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  body: string;
}[] = [
  {
    value: "none",
    icon: Network,
    title: "No mesh",
    body: "The hub stays on the addresses you picked. Fine for a machine everyone who uses it can already reach.",
  },
  {
    value: "coordination",
    icon: Waypoints,
    title: "Join the organization's mesh",
    body: "Ask the coordination server for a mesh key while it authorizes this hub. Whoever accepts the hub decides whether to grant it — nothing extra to fill in here.",
  },
  {
    value: "manual",
    icon: KeyRound,
    title: "Use a key I already have",
    body: "Paste a pre-authorized key from a tailnet you run yourself. Single-use, and it is written into docker-compose.yaml in the deployment folder.",
  },
];

export const MeshStep = () => {
  const { setValue } = useFormContext();
  const values = useWatch() as HubForm;
  const mode = values.meshMode ?? "none";

  return (
    <StepFrame
      icon={Waypoints}
      title="Mesh"
      subtitle="Should this hub join the organization's private network?"
      lead="A Tailscale sidecar runs alongside the gateway and carries its traffic, which puts the hub on the mesh under a name of its own. The addresses you just picked are still the ones clients are told about — the tailnet name only exists once the hub has joined, so you add it from the dashboard afterwards."
    >
      <div className="max-w-xl flex flex-col gap-2">
        {OPTIONS.map((option) => {
          const selected = mode === option.value;
          const Icon = option.icon;
          return (
            <Card
              key={option.value}
              onClick={() =>
                setValue("meshMode", option.value, { shouldValidate: true })
              }
              className={cn(
                "gap-0 py-3 px-4 cursor-pointer border transition-colors",
                selected ? "border-primary bg-primary/5" : "border-border"
              )}
            >
              <div className="flex items-start gap-3">
                <span
                  className={cn(
                    "mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md border",
                    selected
                      ? "border-primary text-primary"
                      : "border-border text-muted-foreground"
                  )}
                >
                  <Icon className="size-3.5" />
                </span>
                <div className="min-w-0">
                  <div className="font-medium">{option.title}</div>
                  <div className="text-sm text-muted-foreground mt-0.5">
                    {option.body}
                  </div>
                </div>
              </div>
            </Card>
          );
        })}

        {mode === "manual" && (
          <div className="mt-3 flex flex-col gap-5">
            <StepField
              label="Mesh auth key"
              hint="A pre-authorized key — tskey-auth-… from Tailscale, or one minted by your own control server."
            >
              <UIField
                name="meshAuthKey"
                type="password"
                autoComplete="off"
                spellCheck="false"
              />
              <ErrorDisplay name="meshAuthKey" className="mt-1" />
            </StepField>

            <AdvancedFields fields={["meshCoordUrl"]}>
              <StepField
                label="Control server"
                hint="The login server the key belongs to. Leave empty for Tailscale's own."
              >
                <UIField
                  name="meshCoordUrl"
                  placeholder="https://mesh.example.org"
                  autoComplete="off"
                  spellCheck="false"
                />
                <ErrorDisplay name="meshCoordUrl" className="mt-1" />
              </StepField>
            </AdvancedFields>
          </div>
        )}

        {mode !== "none" && (
          <Alert className="mt-3 text-xs text-muted-foreground">
            This hub will join as{" "}
            <code className="text-foreground">
              {previewHostname(values.identifier || "hub") || "hub"}
            </code>
            . Start it, find the address the mesh gave it, then add that address and
            authorize again from the dashboard — that is what puts it in front of clients
            that are not on this network. The key is stored in{" "}
            <code>docker-compose.yaml</code>, alongside the other secrets that deployment
            folder already holds.
          </Alert>
        )}

        <ErrorDisplay name="meshMode" className="mt-2" />
      </div>
    </StepFrame>
  );
};
