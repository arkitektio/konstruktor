import { open } from "@tauri-apps/plugin-shell";
import {
  BookOpen,
  Info,
  KeyRound,
  Network,
  TriangleAlert,
  Waypoints,
} from "lucide-react";
import { useFormContext, useWatch } from "react-hook-form";
import { ErrorDisplay } from "../../../components/Error";
import { UIField } from "../../../components/FormInput";
import { Alert } from "../../../components/ui/alert";
import { Button } from "../../../components/ui/button";
import { Card } from "../../../components/ui/card";

import { cn } from "../../../utils";
import { DOCS } from "../../../docs";
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
 * Whether this hub joins a mesh, where its key comes from, and whether the mesh is the
 * *only* way in.
 *
 * A hub advertised only at LAN addresses is only reachable from that LAN. The mesh is
 * the way out: a Tailscale sidecar joins the hub to the organization's tailnet and the
 * gateway is published inside that container's network namespace, so the hub is on the
 * tailnet under a name of its own. The manifest declares a placeholder mesh alias, and
 * the coordination server fills in the tailnet address once the node has joined — so
 * nothing has to be done afterwards.
 *
 * It is asked before ports and addresses because mesh-only makes both moot: nothing is
 * published on this machine, and nothing on its networks is advertised.
 */

const OPTIONS: {
  value: MeshMode;
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  body: string;
}[] = [
  {
    value: "coordination",
    icon: Waypoints,
    title: "Join the organization's mesh",
    body: "Recommended. The coordination server hands out a mesh key while it authorizes this hub — nothing extra to fill in here. Whoever accepts the hub decides whether to grant it.",
  },
  {
    value: "manual",
    icon: KeyRound,
    title: "Use a key I already have",
    body: "Paste a pre-authorized key from a tailnet you run yourself. Single-use, and it is written into docker-compose.yaml in the deployment folder.",
  },
  {
    value: "none",
    icon: Network,
    title: "No mesh",
    body: "The hub is reached only at the addresses you pick next. Fine for a machine everyone who uses it can already reach.",
  },
];

export const MeshStep = () => {
  const { setValue } = useFormContext();
  const values = useWatch() as HubForm;
  const mode = values.meshMode ?? "coordination";
  const meshOnly = mode !== "none" && !!values.meshOnly;

  const choose = (value: MeshMode) => {
    setValue("meshMode", value, { shouldValidate: true });
    // Mesh-only without a mesh would be a hub nothing can reach.
    if (value === "none") setValue("meshOnly", false, { shouldValidate: true });
  };

  return (
    <StepFrame
      icon={Waypoints}
      title="Mesh"
      subtitle="Should this hub join the organization's private network?"
      lead="The mesh is a private network every member of your organization is already on. A small Tailscale container runs next to the gateway and carries its traffic, so the hub gets a name of its own on the mesh and can be reached from anywhere the mesh reaches — no port forwarding, no public address. The coordination server tells clients that name once the hub has joined."
    >
      <div className="max-w-xl flex flex-col gap-2">
        <div className="mb-2">
          <Button size="sm" variant="ghost" onClick={() => open(DOCS.mesh)}>
            <BookOpen className="size-3.5" />
            How the mesh works
          </Button>
        </div>

        {OPTIONS.map((option) => {
          const selected = mode === option.value;
          const Icon = option.icon;
          return (
            <Card
              key={option.value}
              onClick={() => choose(option.value)}
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
            <Alert>
              <Info />
              <div>
                Already running Tailscale on this machine? Nothing changes for it — it
                keeps working as it is. The hub joins through its own container and only
                tunnels the hub's traffic; your machine's Tailscale is never touched.{" "}
                <button
                  type="button"
                  className="underline underline-offset-2 hover:text-foreground"
                  onClick={() => open(DOCS.meshWithTailscale)}
                >
                  Read more
                </button>
              </div>
            </Alert>

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
          <>
            <label className="mt-4 flex items-start gap-3 cursor-pointer">
              <input
                type="checkbox"
                className="mt-1"
                checked={meshOnly}
                onChange={(e) =>
                  setValue("meshOnly", e.target.checked, { shouldValidate: true })
                }
              />
              <div>
                <div className="font-medium text-sm">Mesh only</div>
                <div className="text-sm text-muted-foreground mt-0.5">
                  Open no ports on this machine and advertise nothing on its networks.
                  Clients find the hub on the mesh, and plugin apps running next to it
                  in Docker find it by its name inside the Docker network.
                </div>
              </div>
            </label>

            {meshOnly && (
              <Alert variant="destructive" className="mt-2">
                <TriangleAlert />
                <div>
                  <strong>All traffic goes through the mesh.</strong> Apps and machines
                  on this network that are not on the mesh cannot connect to the hub —
                  not even from this computer's browser, unless it is on the mesh too.
                  Mesh traffic is encrypted and sometimes relayed, so uploads and large
                  images will be slower than over a direct connection on your local
                  network.
                </div>
              </Alert>
            )}

            <Alert className="mt-2 text-xs text-muted-foreground">
              This hub will join as{" "}
              <code className="text-foreground">
                {previewHostname(values.identifier || "hub") || "hub"}
              </code>{" "}
              once it starts. The key is stored in <code>docker-compose.yaml</code>,
              alongside the other secrets that deployment folder already holds.
            </Alert>
          </>
        )}

        <ErrorDisplay name="meshMode" className="mt-2" />
      </div>
    </StepFrame>
  );
};
