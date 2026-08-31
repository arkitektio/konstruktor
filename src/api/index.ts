import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  AdvertisedHost,
  ContainerEngine,
  InstallLine,
  InstallOutcome,
  InstallerId,
  StartTarget,
  BackupEvent,
  BackupManifest,
  BackupReport,
  RestoreEvent,
  RestoreOptions,
  RestorePlan,
  RestoreReport,
  Checkout,
  ComposeFileView,
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
  EngineAnswers,
  HubAnswers,
  HubStatus,
  ImageState,
  UpstreamCheck,
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
 * The verdict is the core's — `DockerProbe.state` — so the CLI's `doctor` and this app
 * can never disagree about it. All this adds is "checking", for before the first probe
 * has come back.
 */
export const dockerState = (probe: DockerProbe | null) => {
  if (!probe) return "checking" as const;
  return probe.state;
};

/** What to call the engine in a sentence. Docker until we know otherwise. */
const ENGINE_NAME: Record<ContainerEngine, string> = {
  docker: "Docker",
  podman: "Podman",
};

export const engineName = (engine: ContainerEngine | null | undefined) =>
  (engine && ENGINE_NAME[engine]) ?? "Docker";

/**
 * Runs one of the core's fixed installers, streaming its output. The id selects a plan
 * written in Rust; nothing typed here is ever executed.
 */
export const installEngine = (
  installer: InstallerId,
  onLine: (line: InstallLine) => void
) => {
  const channel = new Channel<InstallLine>();
  channel.onmessage = onLine;
  return invoke<InstallOutcome>("install_engine", { installer, onLine: channel });
};

export const cancelInstall = () => invoke<void>("cancel_install");

/** Launches the product behind a stopped daemon and returns at once. */
export const startEngine = (target: StartTarget) =>
  invoke<void>("start_engine", { target });

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

/** A free folder in the home directory, named after what is being created. */
export const suggestFolder = (base?: string) =>
  invoke<string | null>("suggest_folder", { base: base ?? null });

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
/**
 * Stop waiting for a device code to be accepted, in whichever of the three flows is
 * waiting. The call that was waiting rejects with "Cancelled." and has written nothing:
 * the folder is only written once the coordination server has accepted the hub.
 */
export const cancelAuthorization = () => invoke<void>("cancel_authorization");

export const createHub = (
  answers: HubAnswers,
  onEvent: (event: CreateEvent) => void
) => {
  const channel = new Channel<CreateEvent>();
  channel.onmessage = onEvent;
  return invoke<string>("create_hub", { answers, onEvent: channel });
};

/**
 * Create a plugin engine: one `jhnnsrs/deployer:next` container with the Docker socket,
 * in its own folder. Streams the same events a hub does — minus the device code, which
 * the app authorization flow will add.
 */
export const createEngine = (
  answers: EngineAnswers,
  onEvent: (event: CreateEvent) => void
) => {
  const channel = new Channel<CreateEvent>();
  channel.onmessage = onEvent;
  return invoke<string>("create_engine", { answers, onEvent: channel });
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
 * Erases a hub's data in place: the containers first, with their volumes, then any
 * database and object storage directories a folder-mode profile names. The folder,
 * `hub_config.yaml`, the credentials,
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
/** One line of a compose command's narration, as Rust streams it. */
export type ComposeLine = { line: string; stderr: boolean };

/**
 * `composeCommand`, with every line of output handed to `onLine` as it is written —
 * what the buttons turn into progress. Resolves to the whole stdout at the end.
 */
export const composeCommandStreamed = (
  path: string,
  action: ComposeAction,
  onLine: (line: ComposeLine) => void
) => {
  const channel = new Channel<ComposeLine>();
  channel.onmessage = onLine;
  return invoke<string>("compose_command_streamed", { path, action, onLine: channel });
};

/**
 * Bring one service up to date: fetch its image when `pull`, then recreate that one
 * container — `--no-deps`, so nothing else in the stack is touched.
 *
 * `pull` is false for an image that is already on disk and merely waiting to be run:
 * applying that must work with no network, so it does not go and ask a registry first.
 */
export const updateService = (
  path: string,
  service: string,
  pull: boolean,
  onLine: (line: ComposeLine) => void
) => {
  const channel = new Channel<ComposeLine>();
  channel.onmessage = onLine;
  return invoke<string>("update_service", { path, service, pull, onLine: channel });
};

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

/**
 * A bug report for one service: its environment, and its log with this deployment's own
 * secrets taken out of it.
 *
 * Assembled in Rust because that is where the deployment's configuration is — the
 * redaction matches the hub's actual credentials rather than guessing at what a secret
 * looks like. See `konstruktor_core::redact`.
 */
export type BugReport = {
  service: string;
  /** Where the service's code lives. `null` for one the profile does not name. */
  repo: string | null;
  /** The prefilled "new issue" page. `null` when there is no repository to file against. */
  issueUrl: string | null;
  title: string;
  /** The whole report as markdown — this is what goes on the clipboard. */
  body: string;
  /** How many distinct secret values were removed from the log. */
  redactions: number;
  /** Why the log is missing, when it is. */
  logError: string | null;
};

export const bugReport = (path: string, service: string) =>
  invoke<BugReport>("bug_report", { path, service });

export const listDeploymentContainers = (path: string) =>
  invoke<{ containers: Container[] }>("list_deployment_containers", { path });

export const restartContainer = (containerId: string) =>
  invoke<void>("restart_container", { containerId });

/**
 * What the local daemon holds for every image this deployment's stack declares.
 *
 * Paired with the containers' `image_id`, this separates "never pulled" from "pulled and
 * running" from "pulled but still waiting for a restart". It says nothing about the
 * registry — that is `checkUpdates` below, which the dashboard runs beside this one.
 */
export const deploymentImages = (path: string) =>
  invoke<ImageState[]>("deployment_images", { path });

/** Asks each image's registry whether its tag moved on since the last pull. Network. */
export const checkUpdates = (path: string) =>
  invoke<UpstreamCheck[]>("check_updates", { path });

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

// --- the compose file, by hand -------------------------------------------------

export const readComposeFile = (path: string) =>
  invoke<ComposeFileView>("read_compose_file", { path });

export const readComposeBackup = (path: string) =>
  invoke<string | null>("read_compose_backup", { path });

/** Saves the file, keeping the previous one as `docker-compose.yaml.bak`. */
export const writeComposeFile = (path: string, contents: string) =>
  invoke<void>("write_compose_file", { path, contents });

/**
 * Docker's own verdict on the file on disk: `null` when it accepts it, otherwise the
 * complaint it printed. Rejects only when the engine could not be asked at all.
 */
export const validateComposeFile = (path: string) =>
  invoke<string | null>("validate_compose_file", { path });

// --- backups ---------------------------------------------------------------------

/** Where a backup started now would land, for the dialog to say before it starts. */
export const backupFolder = (path: string, target: string) =>
  invoke<string>("backup_folder", { path, target });

/**
 * Backs the hub up into `target`, with every line of progress handed to `onEvent`.
 * Resolves to the report once the folder is complete.
 */
export const backupDeployment = (
  path: string,
  target: string,
  onEvent: (event: BackupEvent) => void
) => {
  const channel = new Channel<BackupEvent>();
  channel.onmessage = onEvent;
  return invoke<BackupReport>("backup_deployment", { path, target, onEvent: channel });
};

// --- restore -----------------------------------------------------------------------

export const readBackupManifest = (backup: string) =>
  invoke<BackupManifest>("read_backup_manifest", { backup });

/** The comparison, for the review step. Touches nothing. */
export const restorePlan = (path: string, backup: string, options: RestoreOptions) =>
  invoke<RestorePlan>("restore_plan", {
    path,
    backup,
    method: options.method,
    restorePostgres: options.restore_postgres,
    restoreMinio: options.restore_minio,
  });

/** The restore itself; resolves to the report once the health check is done. */
export const restoreDeployment = (
  path: string,
  backup: string,
  options: RestoreOptions,
  onEvent: (event: RestoreEvent) => void
) => {
  const channel = new Channel<RestoreEvent>();
  channel.onmessage = onEvent;
  return invoke<RestoreReport>("restore_deployment", {
    path,
    backup,
    method: options.method,
    restorePostgres: options.restore_postgres,
    restoreMinio: options.restore_minio,
    onEvent: channel,
  });
};
