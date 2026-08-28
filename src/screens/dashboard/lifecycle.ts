/**
 * A deployment's life, derived from what is already on disk and in the daemon.
 *
 * A hub does not report its own state anywhere — there is no Konstruktor label on the
 * stack, no status file, no daemon to ask. Everything below is inferred from three
 * sources the dashboard already holds: the registry record (when it was created, when
 * its files were last written), the profile and credentials (`HubStatus`), and the
 * containers plus local images Docker reports. Keeping the inference here, away from
 * JSX, is what makes it testable — and the rules are the part worth being sure about.
 */

import type { Container, HubStatus, ImageState } from "../../api";

// --- is it running ----------------------------------------------------------

/**
 * `never` and `stopped` are deliberately apart: a deployment with no containers at all
 * has nothing to restart, while one whose containers merely exited needs starting again.
 *
 * The name is about the containers, not about history — `compose down` removes them from
 * a hub that ran for months — which is why the copy says "no containers" rather than
 * claiming the deployment was never brought up.
 */
export type RunState = "running" | "partial" | "stopped" | "never";

export const RUN_STATE_LABEL: Record<RunState, string> = {
  running: "Running",
  partial: "Partly running",
  stopped: "Stopped",
  never: "No containers",
};

export type RunSummary = {
  state: RunState;
  running: number;
  total: number;
};

/**
 * Counted over **every** container in the compose project, not just the arkitekt
 * services: Postgres, Redis, MinIO and the gateway are what the services stand on, and a
 * summary that ignored them would report a hub as running while its database was down.
 */
export const runSummary = (containers: Container[]): RunSummary => {
  const total = containers.length;
  const running = containers.filter((c) => c.state === "running").length;

  if (total === 0) return { state: "never", running: 0, total: 0 };
  if (running === 0) return { state: "stopped", running, total };
  return { state: running === total ? "running" : "partial", running, total };
};

// --- is there an update -----------------------------------------------------

/**
 * What the local daemon holds for one compose service, against what is running.
 *
 * `pulled` is the interesting one: `docker compose pull` moves a tag to a newer image
 * without touching anything already running, so a hub can sit for weeks on an update it
 * has already downloaded. Nothing here asks a registry: whether a *newer* image exists
 * upstream is a question the daemon cannot answer without a pull.
 */
export type UpdateState =
  | "current"
  | "pulled"
  | "missing"
  | "unknown";

export type ServiceUpdate = {
  /** The compose service, which is the profile's `host` for that service. */
  service: string;
  image: string | null;
  tag: string | null;
  state: UpdateState;
  /** When the image the daemon now resolves the tag to was built. */
  imageCreated: string | null;
};

const tagOf = (image: string | null | undefined): string | null => {
  if (!image) return null;
  const last = image.split("/").pop() ?? image;
  const colon = last.lastIndexOf(":");
  return colon === -1 ? null : last.slice(colon + 1);
};

/**
 * Lines up each declared image with the container running it.
 *
 * The comparison is on image **ids**, not tags: a tag is a moving name, so `next` against
 * `next` proves nothing, while the id a container was created from against the id the tag
 * resolves to now is exactly the question "has this been updated underneath us".
 */
export const serviceUpdates = (
  images: ImageState[],
  containers: Container[]
): ServiceUpdate[] =>
  images.map((image) => {
    const container = containers.find((c) => c.service === image.service);

    const state: UpdateState = !image.present
      ? "missing"
      : !container
        ? "unknown"
        : container.image_id && image.image_id && container.image_id !== image.image_id
          ? "pulled"
          : "current";

    return {
      service: image.service,
      image: image.image,
      tag: tagOf(image.image),
      state,
      imageCreated: image.created,
    };
  });

export type UpdateSummary = {
  /** Downloaded but not yet running — a restart applies these. */
  pulled: ServiceUpdate[];
  /** Declared by the profile but never fetched — a pull is needed first. */
  missing: ServiceUpdate[];
};

export const updateSummary = (updates: ServiceUpdate[]): UpdateSummary => ({
  pulled: updates.filter((u) => u.state === "pulled"),
  missing: updates.filter((u) => u.state === "missing"),
});

// --- the rail ---------------------------------------------------------------

export type StageState = "done" | "waiting" | "attention";

export type Stage = {
  key: "created" | "authorized" | "generated" | "started";
  label: string;
  state: StageState;
  /** The one line under the label — a date, a count, what is missing. */
  detail: string;
};

const asDate = (value: string | null | undefined): Date | null => {
  if (!value) return null;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
};

export const formatDate = (value: string | null | undefined): string | null => {
  const date = asDate(value);
  return date
    ? date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })
    : null;
};

/**
 * The four stages every hub passes through, in order, each with the one fact that
 * answers "did this happen, and when".
 *
 * `generated` is `attention` rather than `done` when the folder's service configs were
 * last written *before* the hub was last authorized: re-authorizing can move the JWKS URL
 * the services verify tokens against, and configs older than that authorization no longer
 * describe the hub the coordination server knows about.
 */
export const stages = (
  deployment: { createdAt: string; lastGeneratedAt?: string },
  status: HubStatus | undefined,
  run: RunSummary
): Stage[] => {
  const created = formatDate(deployment.createdAt);

  const authorizedAt = asDate(status?.authorized_at);
  const generatedAt = asDate(deployment.lastGeneratedAt);
  const stale =
    authorizedAt !== null && generatedAt !== null && generatedAt < authorizedAt;

  return [
    {
      key: "created",
      label: "Created",
      state: "done",
      detail: created ?? "date unknown",
    },
    {
      key: "authorized",
      label: "Authorized",
      state: status?.authorized ? "done" : "attention",
      detail: status?.authorized
        ? (status.identifier ?? status.profile.config.coord_server)
        : "Not authorized against a coordination server",
    },
    {
      key: "generated",
      label: "Configured",
      state: !generatedAt ? "waiting" : stale ? "attention" : "done",
      detail: !generatedAt
        ? "No generation recorded"
        : stale
          ? "Written before the last authorization — regenerate"
          : (formatDate(deployment.lastGeneratedAt) ?? ""),
    },
    {
      key: "started",
      label: "Started",
      state:
        run.state === "running"
          ? "done"
          : run.state === "partial"
            ? "attention"
            : "waiting",
      detail:
        run.state === "never"
          ? "No containers — nothing is up right now"
          : `${run.running} of ${run.total} containers running`,
    },
  ];
};
