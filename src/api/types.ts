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

export type DockerState = "ready" | "missing" | "no-compose" | "no-daemon";

export type DockerProbe = {
  cli: boolean;
  cli_version: string | null;
  compose: boolean;
  compose_version: string | null;
  daemon: boolean;
  api_version: string | null;
  memory: number | null;
  error: string | null;
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
};

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
