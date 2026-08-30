/**
 * The shapes `konstruktor-core` sends across the IPC boundary.
 *
 * These mirror Rust structs serialized by serde, which uses field names verbatim — so
 * everything here is snake_case, unlike the rest of the frontend. Keeping the wire shape
 * honest is worth the inconsistency: a renamed field would fail silently as `undefined`.
 */

export const SERVICE_IDS = [
  "rekuest",
  "mikro",
  "fluss",
  "kabinet",
  "kraph",
  "elektro",
  "alpaka",
  "lovekit",
] as const;

export type ServiceId = (typeof SERVICE_IDS)[number];

export type ServiceMeta = {
  id: ServiceId;
  name: string;
  description: string;
  /** What it is actually for — the paragraph the picker shows beside the list. */
  purpose: string;
  /** Pre-ticked when nothing else is said. */
  default: boolean;
  /** Lovekit has no published image, so ticking it would change nothing. */
  emitted: boolean;
};

/**
 * What an address or a name is, in enough detail to decide how far it reaches.
 *
 * Mirrors `HostCategory` in `konstruktor-core::hosts`. The core maps these onto the four
 * scopes the coordination server accepts, and decides which are worth advertising — see
 * `usable` and the presets. Nothing here re-derives either.
 */
export type HostCategory =
  | "loopback"
  | "private"
  | "mesh"
  | "other-mesh"
  | "public"
  | "virtual"
  | "link-local"
  | "mdns-name"
  | "bare-hostname"
  | "fqdn"
  | "verified-fqdn";

export type UnusableReason = "virtual-interface" | "link-local" | "unspecified";

export type HostCandidate = {
  value: string;
  kind: HostCategory;
  interface: string;
  /** Pre-ticked by the presets. Implies `usable`. */
  recommended: boolean;
  /** False for addresses that exist but cannot help a peer reach this hub. */
  usable: boolean;
  unusable_reason: UnusableReason | null;
  /** What this address is, in words, written by the core so we keep no second copy. */
  summary: string;
};

export type ReachPresetId = "local-only" | "this-network" | "public";

export type ReachPreset = {
  id: ReachPresetId;
  label: string;
  description: string;
  /** The candidate values this preset selects. */
  values: string[];
};

/** Everything the address step needs, in one call. */
export type HostDiscovery = {
  candidates: HostCandidate[];
  presets: ReachPreset[];
};

export type AdvertisedHost = { host: string; kind: HostCategory };

/**
 * Whether something outside actually connected back.
 *
 * Deliberately not a boolean, and deliberately not the same fact as "the internet sees
 * you at this address" — an address matching this machine's egress IP says nothing about
 * whether a port is open.
 */
export type ProbeResult =
  | { result: "reachable"; status: number }
  | { result: "unreachable"; reason: string }
  | { result: "not-checked" };

export type DockerState =
  | "ready"
  | "missing"
  | "no-compose"
  | "no-daemon"
  | "too-old";

export type DockerProbe = {
  cli: boolean;
  cli_version: string | null;
  compose: boolean;
  compose_version: string | null;
  daemon: boolean;
  api_version: string | null;
  memory: number | null;
  error: string | null;
  /**
   * Which engine answered. Podman speaks the same subcommands, so everything else here
   * means the same thing either way — this only decides what to call it on screen.
   * `null` when nothing was found at all.
   */
  engine: ContainerEngine | null;
  /** Which product is behind it — what to start, or update. */
  brand: EngineBrand;
  /** The OS the probe ran on. The remedies are already chosen for it. */
  platform: Platform;
  /** The verdict, decided in the core. Nothing here re-derives it. */
  state: DockerState;
  /** What to do about it, primary first. Empty when ready. */
  remedies: Remedy[];
};

/** The container engines Konstruktor knows how to drive. */
export type ContainerEngine = "docker" | "podman";

/** Mirrors `EngineBrand` in `konstruktor-core::engine_probe`. */
export type EngineBrand =
  | "docker-desktop"
  | "colima"
  | "orb-stack"
  | "rancher-desktop"
  | "podman-desktop"
  | "native"
  | "unknown";

export type Platform = "macos" | "windows" | "linux" | "other";

/** The fixed installers the app can run — see `konstruktor-core::remedy`. */
export type InstallerId = "brew-colima" | "brew-compose-plugin" | "winget-rancher-desktop";

export type StartTarget =
  | "colima"
  | "docker-desktop"
  | "orb-stack"
  | "rancher-desktop"
  | "podman-machine";

/**
 * One thing to do, as the core worded it. The panel renders each kind differently —
 * a link, a code block with a copy button, an install button with a log, a start
 * button, or a sentence — and invents no wording of its own.
 */
export type RemedyStep =
  | { kind: "open-url"; label: string; url: string }
  | { kind: "copy-command"; label: string; command: string }
  | { kind: "run-installer"; label: string; installer: InstallerId }
  | { kind: "start-engine"; label: string; target: StartTarget }
  | { kind: "note"; text: string };

export type Remedy = {
  title: string;
  body: string;
  steps: RemedyStep[];
  /** The one we recommend. Always the first; the rest are alternatives. */
  primary: boolean;
};

/** One line of an installer's output. `stage` lines are headings, not output. */
export type InstallLine = { line: string; stderr: boolean; stage: boolean };

export type InstallOutcome = {
  ok: boolean;
  needsReboot: boolean;
  cancelled: boolean;
  message: string | null;
};

/**
 * Git, which is optional in a way Docker is not: a hub runs published images and never
 * needs it. It only matters for a dev hub, whose services run from a checkout.
 */
export type GitProbe = {
  cli: boolean;
  cli_version: string | null;
};

/**
 * One service's source checkout in a dev hub's `mounts/` folder.
 *
 * Every field is answered on its own and a failure is a field, not an exception: a
 * checkout somebody deleted shows up saying so rather than taking the page with it.
 */
export type Checkout = {
  service: string;
  path: string;
  repo: string;
  /** The branch HEAD is on. Null on a detached HEAD, or when there is no repository. */
  branch: string | null;
  head: string | null;
  detached: boolean;
  /**
   * Tracked files differ from HEAD. Untracked files do not count — the containers write
   * `__pycache__` into the mount, and counting that would refuse every switch forever.
   */
  dirty: boolean;
  error: string | null;
};

export type MeshMode = "none" | "coordination" | "manual";

/** Everything the wizard collects. One object, one call. */
export type HubAnswers = {
  dir: string;
  name: string;
  coord_server: string;
  identifier: string;
  description?: string | null;
  rekuest_server: string;
  services: ServiceId[];
  http_port: number;
  https_port: number;
  ssl: boolean;
  domain?: string | null;
  global_admin: string;
  global_admin_password?: string | null;
  global_description?: string | null;
  hosts: AdvertisedHost[];
  /** Of `hosts`, the ones an external probe reached. Empty unless somebody checked. */
  reachable_hosts: string[];
  mesh_mode: MeshMode;
  mesh_auth_key?: string | null;
  mesh_coord_url?: string | null;
  start: boolean;
  /**
   * Every service's source, checked out and mounted into its container. The CLI's
   * `--dev`; the wizard asks per service through `service_options` instead.
   */
  dev_hub: boolean;
  /** The branch to check out. Null means each repository's own default branch. */
  dev_branch?: string | null;
  /** What was asked of one service in particular. Absent services take the default. */
  service_options?: Partial<Record<ServiceId, ServiceOptions>>;
  /** Where the database and object storage keep their data. */
  storage: StorageMode;
};

/**
 * Where a hub's database and object storage live.
 *
 * `docker-volumes` is the default and the fast one: a named volume sits inside the
 * engine's own VM on macOS and Windows, with none of the file-sharing overhead a bind
 * mount pays. `deployment-folder` bind-mounts `./db_data` and `./minio_data` into the
 * folder instead, so the data is a directory you can see — and pays for it on every
 * write.
 */
export type StorageMode = "docker-volumes" | "deployment-folder";

/** The per-service answers the gear on the services step collects. */
export type ServiceOptions = {
  /** Run this one from a checkout of its repository, mounted over the image. Needs git. */
  from_source: boolean;
  /** The branch to check out. Null means the repository's own default. */
  branch?: string | null;
  /** Django's debug mode, for this service alone. */
  debug?: boolean;
  /** Alpaka only: where its language models come from. */
  ollama?: OllamaChoice | null;
  /** Kabinet only: the app repositories this hub offers, replacing the default pair. */
  repositories?: string[] | null;
};

/**
 * Where Alpaka's models come from.
 *
 * `run_locally` adds an Ollama container to the generated stack; otherwise `url` names
 * one that already exists.
 */
export type OllamaChoice = {
  run_locally: boolean;
  url?: string | null;
};

/**
 * Everything a plugin engine needs. An engine is one container — the deployer, with the
 * Docker socket — so it is asked far less than a hub: no services, no ports, no
 * addresses to advertise, no mesh.
 */
export type EngineAnswers = {
  dir: string;
  name: string;
  coord_server: string;
  identifier: string;
  description?: string | null;
  start: boolean;
};

/**
 * Progress from a running `create_hub`. Tagged by `event`, so a switch is exhaustive.
 * The device code arrives as `staged` — which is why creating a hub can be one call
 * rather than a wizard step whose result can go stale.
 */
export type CreateEvent =
  | { event: "checking-docker" }
  | { event: "building" }
  | {
      event: "staged";
      user_code: string;
      verification_uri_complete: string;
      expires_in: number;
    }
  | { event: "waiting"; polls: number; seconds_left: number }
  | { event: "granted"; mesh_key: boolean }
  | { event: "writing"; file: string }
  | { event: "cloning"; service: string; repo: string; branch: string | null }
  | { event: "starting" }
  | { event: "log"; line: string }
  | { event: "done"; path: string };

export type DeploymentRecord = {
  id: string;
  name: string;
  path: string;
  kind: string;
  project: string;
  createdAt: string;
  lastGeneratedAt?: string;
  coordServer?: string;
  identifier?: string;
};

export type FolderReport = { ok: boolean; message: string };

export type WellKnownFakts = {
  name?: string | null;
  version?: string | null;
  description?: string | null;
  issuer?: string | null;
  hub_authorization_endpoint?: string | null;
};

export type ServiceView = {
  id: ServiceId;
  name: string;
  host: string;
  /** Where a browser reaches it through the gateway. */
  url: string;
  /** The image the profile pins this service to, e.g. `jhnnsrs/rekuest:next`. */
  image: string | null;
  /** That image's tag on its own — the service's release channel. */
  tag: string | null;
};

/**
 * The release channel a hub follows, read off the images its services are pinned to.
 *
 * Nothing in the profile names a channel: the channel *is* the set of tags, and those are
 * per-service. `tag` is filled only when every service agrees; more than one entry in
 * `tags` means the hub is mixed, and the UI says so rather than picking one.
 */
export type ChannelView = {
  tag: string | null;
  tags: string[];
};

/** What the local Docker daemon holds for one image the stack declares. */
export type ImageState = {
  /** The reference as the compose file spells it. */
  image: string;
  /** The compose service that runs it. */
  service: string;
  /** Whether the daemon has it at all. `false` means nothing has pulled it yet. */
  present: boolean;
  /** The id the tag resolves to now — compared against a container's `image_id`. */
  image_id: string | null;
  created: string | null;
  /** Registry digests this image was pulled as, e.g. `jhnnsrs/rekuest@sha256:…`. */
  repo_digests: string[];
};

/** What a registry says about one image's tag, against what the engine holds. */
export type UpstreamCheck = {
  service: string;
  image: string;
  state: "current" | "newer" | "missing" | "unknown";
  remote_digest: string | null;
  error: string | null;
};

/** What the dashboard reads out of a deployment folder. */
export type HubStatus = {
  profile: { version: string; kind: string; backend: string; config: HubConfigView };
  authorized: boolean;
  identifier: string | null;
  authorized_at: string | null;
  gateway_url: string;
  admin_user: string;
  admin_password: string;
  services: ServiceView[];
  mesh_hostname: string | null;
  /** The port an alias advertises, computed where the manifest computes it. */
  advertised_port: number;
  /** What this hub last told the coordination server it was reachable at. */
  advertised_hosts: AdvertisedHost[];
  channel: ChannelView;
  /** Where the database and object storage live. */
  storage: StorageMode;
};

/** Only the parts of the profile the UI reads; the rest round-trips untouched. */
export type HubConfigView = {
  coord_server: string;
  rekuest_server: string;
  domain: string | null;
  internal_network: string;
  gateway: {
    exposed_http_port: number | null;
    exposed_https_port: number | null;
    ssl: boolean;
  };
};

/**
 * No `down-volumes`. It named an action that removed nothing — the stack keeps its data
 * in bind mounts and declares no named volumes — and having it in the vocabulary is what
 * let the menu promise something it never did. Erasing data is `purgeDeploymentData`.
 */
export type ComposeAction = "up" | "stop" | "down" | "pull" | "ps" | "logs";

/**
 * What deleting a deployment would take with it, worked out before the user is asked.
 *
 * `checkouts` and `was_authorized` exist so the confirmation can name what it cannot
 * undo: a dev hub's `mounts/` trees may hold work that is nowhere else, and an authorized
 * hub keeps an identifier on a coordination server that a local delete cannot revoke.
 */
export type DeletionPlan = {
  path: string;
  name: string;
  checkouts: string[];
  was_authorized: boolean;
  /**
   * The data directories, resolved. Named by the core rather than assumed by the UI:
   * `db_data` and `minio_data` are defaults, not constants, and a profile in the wild can
   * point somewhere else.
   */
  data_dirs: string[];
  skipped: SkippedMount[];
  /** On a mesh: the tailnet state is a volume, and the key that joined it was single-use. */
  on_a_mesh: boolean;
  /** Where the data is, which decides whether `down --volumes` or `data_dirs` holds it. */
  storage: StorageMode;
};

/** What a delete actually managed to remove, step by step. */
export type Deletion = {
  path: string;
  stack_removed: boolean;
  folder_removed: boolean;
  forgotten: boolean;
};

/** A mount the purge left alone, and why. */
export type SkippedMount = {
  mount: string;
  /** Already in words — a front end does not need to know the reasons by name. */
  explanation: string;
};

/**
 * What a data purge removed, and what it deliberately did not.
 *
 * Not `compose down --volumes`: the database and object storage are bind mounts inside
 * the deployment folder and the stack declares no named volumes, so that command took no
 * data with it at all.
 */
export type DataPurge = {
  path: string;
  stack_removed: boolean;
  /** The data directories that are gone, as absolute paths. */
  removed: string[];
  skipped: SkippedMount[];
};

/** The compose file as the editor reads it. */
export type ComposeFileView = {
  contents: string;
  /** What the generator would write from the profile today; empty for an engine. */
  generated: string;
  /** A `docker-compose.yaml.bak` from an earlier save is there to go back to. */
  has_backup: boolean;
};

/** One thing the backup says while it runs; `step` groups the lines by part. */
export type BackupEvent =
  | { event: "step"; step: string; title: string }
  | { event: "line"; step: string; line: string; stderr: boolean }
  | { event: "skipped"; step: string; reason: string };

/** What a finished backup folder holds. */
export type BackupReport = {
  path: string;
  taken_at: number;
  storage: StorageMode;
  dumped: boolean;
  postgres_copied: boolean;
  minio_copied: boolean;
  deployment_files: string[];
  warnings: string[];
};

/** `manifest.json`: what a backup is a backup of. */
export type BackupManifest = {
  format: number;
  konstruktor_version: string;
  taken_at: number;
  storage: StorageMode;
  hub: { identifier: string | null; coord_server: string; project: string; path: string };
  services: {
    id: ServiceId;
    host: string;
    image: string;
    image_id: string | null;
    repo_digests: string[];
    db: string;
  }[];
  infrastructure: { service: string; image: string; image_id: string | null }[];
  postgres: { user: string; server_version: string | null };
  contents: {
    dumped: boolean;
    postgres_copied: boolean;
    minio_copied: boolean;
    deployment_files: string[];
    warnings: string[];
  };
};

/** How the database is put back: replay the SQL dump, or copy `PGDATA` byte for byte. */
export type DbMethod = "dump" | "raw";

export type Verdict =
  | "same"
  | "different-tag"
  | "different-build"
  | "missing-in-target"
  | "not-resolvable";

export type ServiceComparison = {
  id: ServiceId;
  host: string;
  backup_image: string;
  backup_image_id: string | null;
  deployed_image: string | null;
  deployed_image_id: string | null;
  verdict: Verdict;
};

/** What restoring a backup into a hub would mean — shown before anything is touched. */
export type RestorePlan = {
  manifest: BackupManifest;
  same_hub: boolean;
  target_identifier: string | null;
  target_storage: StorageMode;
  services: ServiceComparison[];
  extra_in_target: ServiceId[];
  db: { service: string; backup_image: string; deployed_image: string; verdict: Verdict };
  target_postgres_version: string | null;
  available: { dump: boolean; postgres_raw: boolean; minio: boolean };
  /** Why it cannot go ahead as asked; empty means it can. */
  blocking: string[];
  warnings: string[];
};

export type RestoreOptions = {
  method: DbMethod;
  restore_postgres: boolean;
  restore_minio: boolean;
};

export type RestoreEvent =
  | { event: "step"; step: string; title: string }
  | { event: "line"; step: string; line: string; stderr: boolean }
  | { event: "skipped"; step: string; reason: string }
  | { event: "checked"; service: string; healthy: boolean; detail: string };

/** One service's verdict after the restore. */
export type ServiceHealth = {
  service: string;
  container_state: string | null;
  restarts_seen: boolean;
  http_status: number | null;
  url: string | null;
  healthy: boolean;
  detail: string;
};

export type RestoreReport = {
  path: string;
  backup: string;
  method: DbMethod;
  postgres_restored: boolean;
  minio_restored: boolean;
  psql_errors: number;
  health: ServiceHealth[];
  all_healthy: boolean;
  left_running: boolean;
  warnings: string[];
};
