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
  /** Pre-ticked when nothing else is said. */
  default: boolean;
  /** Lovekit has no published image, so ticking it would change nothing. */
  emitted: boolean;
};

export type HostKind = "hostname" | "private" | "public";

export type HostCandidate = {
  value: string;
  kind: HostKind;
  interface: string;
  /** Addresses are reliable; resolved names are offered but not pre-selected. */
  recommended: boolean;
};

export type AdvertisedHost = { host: string; kind: HostKind };

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
  mesh_mode: MeshMode;
  mesh_auth_key?: string | null;
  mesh_coord_url?: string | null;
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

export type ComposeAction =
  | "up"
  | "stop"
  | "down"
  | "down-volumes"
  | "pull"
  | "ps"
  | "logs";
