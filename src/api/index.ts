import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  AdvertisedHost,
  ComposeAction,
  CreateEvent,
  DeploymentRecord,
  DockerProbe,
  FolderReport,
  HostCandidate,
  HubAnswers,
  HubStatus,
  ServiceMeta,
  WellKnownFakts,
} from "./types";

/**
 * The whole backend, in one file.
 *
 * Every function here is a typed `invoke` into `konstruktor-core` — the same code the
 * `konstruktor` command line runs. The frontend generates nothing, authorizes nothing and
 * writes nothing itself; if a capability is missing, it belongs in the core, not here.
 */

export * from "./types";

// --- docker -----------------------------------------------------------------

export const probeDocker = () => invoke<DockerProbe>("probe_docker");

/**
 * Docker reduced to the one thing the UI decides on: what to tell the user next.
 *
 * The three failures stay apart because their remedies differ — a missing binary is a
 * download, a missing plugin is a newer Docker, a silent daemon is "start Docker".
 */
export const dockerState = (probe: DockerProbe | null) => {
  if (!probe) return "checking" as const;
  if (!probe.cli) return "missing" as const;
  if (!probe.compose) return "no-compose" as const;
  if (!probe.daemon) return "no-daemon" as const;
  return "ready" as const;
};

// --- the machine ------------------------------------------------------------

/** The addresses worth advertising, already classified and ordered. */
export const hostCandidates = () => invoke<HostCandidate[]>("host_candidates");

// --- creating a hub ---------------------------------------------------------

export const serviceCatalog = () => invoke<ServiceMeta[]>("service_catalog");

export const suggestFolder = () => invoke<string | null>("suggest_folder");

export const inspectFolder = (path: string) =>
  invoke<FolderReport>("inspect_folder", { path });

export const identifierFromFolder = (path: string) =>
  invoke<string>("identifier_from_folder", { path });

export const discoverServer = (server: string) =>
  invoke<WellKnownFakts>("discover_server", { server });

export const previewHubFiles = (answers: HubAnswers) =>
  invoke<string[]>("preview_hub_files", { answers });

/**
 * Build the profile, authorize it, write the folder, start the stack — one call, with
 * progress streamed back as it happens.
 *
 * Because authorization lives inside this call rather than in a wizard step, its result
 * cannot go stale against answers that changed afterwards; the whole "authorize again"
 * mechanism the wizard used to need is gone.
 */
export const createHub = (
  answers: HubAnswers,
  onEvent: (event: CreateEvent) => void
) => {
  const channel = new Channel<CreateEvent>();
  channel.onmessage = onEvent;
  return invoke<string>("create_hub", { answers, onEvent: channel });
};

// --- the registry, shared with the command line -----------------------------

export const listDeployments = () => invoke<DeploymentRecord[]>("list_deployments");

export const forgetDeployment = (id: string) =>
  invoke<void>("forget_deployment", { id });

export const hubStatus = (path: string) => invoke<HubStatus>("hub_status", { path });

/**
 * Re-authorize a hub that already exists: add services, move it to another network, or
 * tell the coordination server about the tailnet address it only got once it had joined.
 *
 * The profile is reused verbatim — its secrets are what the running services already
 * trust — and the service configs are regenerated afterwards, because the JWKS URL they
 * verify tokens against may have moved.
 */
export const reauthorizeHub = (
  options: {
    path: string;
    coordServer: string;
    identifier: string;
    description?: string | null;
    hosts: AdvertisedHost[];
    requestAuthKey: boolean;
  },
  onEvent: (event: CreateEvent) => void
) => {
  const channel = new Channel<CreateEvent>();
  channel.onmessage = onEvent;
  return invoke<void>("reauthorize_hub", {
    path: options.path,
    coordServer: options.coordServer,
    identifier: options.identifier,
    description: options.description ?? null,
    hosts: options.hosts,
    requestAuthKey: options.requestAuthKey,
    onEvent: channel,
  });
};

// --- folders ----------------------------------------------------------------

export const canonicalizePath = (path: string) =>
  invoke<string>("canonicalize_path", { path });

export const prepareDeploymentDir = (path: string) =>
  invoke<{ path: string; created: boolean }>("prepare_deployment_dir", { path });

export const discardEmptyDir = (path: string) =>
  invoke<void>("discard_empty_dir", { path });

export const allowDeploymentDir = (path: string) =>
  invoke<void>("allow_deployment_dir", { path });

// --- docker compose ---------------------------------------------------------

/**
 * Runs one compose subcommand in a deployment folder and returns its output.
 *
 * Buffered rather than streamed: every one of these runs to completion, and nothing in
 * the UI displays output while a command is still going.
 */
export const composeCommand = (
  path: string,
  action: ComposeAction,
  options: { service?: string; tail?: number } = {}
) =>
  invoke<string>("compose_command", {
    path,
    action,
    service: options.service ?? null,
    tail: options.tail ?? null,
  });

export const listDeploymentContainers = (path: string) =>
  invoke<{ containers: Container[] }>("list_deployment_containers", { path });

export const restartContainer = (containerId: string) =>
  invoke<void>("restart_container", { containerId });

export type Container = {
  id: string | null;
  names: string[] | null;
  image: string | null;
  status: string | null;
  state: string | null;
  service: string | null;
};
