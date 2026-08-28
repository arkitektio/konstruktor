import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  AdvertisedHost,
  Checkout,
  ComposeAction,
  CreateEvent,
  DataPurge,
  Deletion,
  DeletionPlan,
  DeploymentRecord,
  DockerProbe,
  GitProbe,
  FolderReport,
  HostDiscovery,
  HubAnswers,
  HubStatus,
  ImageState,
  ProbeResult,
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

/**
 * Whether git is on this machine. Deliberately its own probe rather than a field on the
 * Docker one: the remedies differ, and so does the consequence — no Docker means no
 * deployment at all, while no git only means no dev hub.
 */
export const probeGit = () => invoke<GitProbe>("probe_git");

// --- a dev hub's source checkouts -------------------------------------------

/**
 * The checkouts this deployment keeps under `mounts/`.
 *
 * An empty list is the answer for every ordinary hub, which is why nothing else has to
 * ask whether a deployment is a dev hub: there is either something to switch branches in
 * or there is not.
 */
export const deploymentCheckouts = (path: string) =>
  invoke<Checkout[]>("deployment_checkouts", { path });

/** The branches one checkout could move to. Fetches first, so it is current. */
export const checkoutBranches = (path: string, service: string) =>
  invoke<string[]>("checkout_branches", { path, service });

/**
 * Move one checkout onto another branch, and read back what it became.
 *
 * Refused over uncommitted work rather than forced — the point of a dev hub is that the
 * checkout holds work somebody is doing. The container goes on running whatever it
 * loaded until the stack is recreated.
 */
export const switchCheckoutBranch = (
  path: string,
  service: string,
  branch: string
) => invoke<Checkout>("switch_checkout_branch", { path, service, branch });

// --- the machine ------------------------------------------------------------

/**
 * The addresses worth advertising, classified and ordered, with the reach presets
 * already resolved against them.
 */
export const hostCandidates = (mesh?: {
  /** The tailnet this hub is on, when the coordination server declares one. */
  domain?: string | null;
  /** The name this hub takes on that tailnet, out of its own mesh config. */
  hostname?: string | null;
}) =>
  invoke<HostDiscovery>("host_candidates", {
    meshDomain: mesh?.domain ?? null,
    meshHostname: mesh?.hostname ?? null,
  });

/**
 * The tailnet a coordination server runs, if it declares one.
 *
 * Without it, a tailnet address on this machine cannot be told apart from one on the
 * personal tailnet most laptops are already on, so the address step calls every such
 * address "another tailscale" rather than offering it as the hub's mesh.
 */
export const meshDomain = (server: string) =>
  invoke<string | null>("mesh_domain", { server });

/**
 * What address the internet sees this machine as.
 *
 * Only ever called with an endpoint the user configured: this is the one request
 * konstruktor makes to a host they did not name, and it tells that host their IP.
 */
export const egressIdentity = (endpoint: string) =>
  invoke<string>("egress_identity", { endpoint });

/**
 * Asks a configured prober to connect back to one advertised address.
 *
 * A different question from {@link egressIdentity}, and the only one that may mark an
 * alias public. With no prober configured the answer is `not-checked`, never a failure.
 */
export const probeReachability = (
  prober: string,
  host: string,
  port: number,
  ssl: boolean
) => invoke<ProbeResult>("probe_reachability", { prober, host, port, ssl });

/**
 * An admin account in one running service, made after the fact.
 *
 * Per service because each keeps its own database and its own admin site. The container
 * has to be up: this is `docker compose exec`, not a file that gets written.
 */
export const createSuperuser = (
  path: string,
  service: string,
  username: string,
  password: string,
  email?: string
) =>
  invoke<string>("create_superuser", {
    path,
    service,
    username,
    password,
    email: email ?? null,
  });

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

/** What deleting this deployment would take with it. Nothing is removed by asking. */
export const planDeletion = (id: string) =>
  invoke<DeletionPlan>("plan_deletion", { id });

/**
 * Deletes a deployment outright: containers, volumes, folder, registry entry.
 *
 * By id rather than by path — the core resolves and guards the folder itself, so the only
 * thing this can ever delete is a deployment Konstruktor already lists.
 */
export const deleteDeployment = (id: string) =>
  invoke<Deletion>("delete_deployment", { id });

/**
 * Erases a hub's data in place: the containers first, then the bind-mounted database and
 * object storage directories. The folder, `hub_config.yaml`, the credentials,
 * `docker-compose.yaml` and `configs/` all stay, so the hub starts again empty.
 *
 * By id rather than by path, like `deleteDeployment` and for the same reason — the core
 * resolves and guards every directory it removes.
 */
export const purgeDeploymentData = (id: string) =>
  invoke<DataPurge>("purge_deployment_data", { id });

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
    /** Of `hosts`, the ones a probe reached. Only these may be marked public. */
    reachableHosts: string[];
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
    reachableHosts: options.reachableHosts,
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

/**
 * What the local daemon holds for every image this deployment's stack declares.
 *
 * Paired with the containers' `image_id`, this separates "never pulled" from "pulled and
 * running" from "pulled but still waiting for a restart". It says nothing about the
 * registry: whether something newer exists upstream is a question nothing here asks.
 */
export const deploymentImages = (path: string) =>
  invoke<ImageState[]>("deployment_images", { path });

export type Container = {
  id: string | null;
  names: string[] | null;
  image: string | null;
  /**
   * The image id the container was created from. It stops matching the tag's current id
   * as soon as a newer image is pulled over that tag.
   */
  image_id: string | null;
  status: string | null;
  state: string | null;
  service: string | null;
};
