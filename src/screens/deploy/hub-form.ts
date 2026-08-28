import { z } from "zod";
import type {
  AdvertisedHost,
  MeshMode,
  ServiceId,
  ServiceOptions,
} from "../../api";

/**
 * The wizard's form shape.
 *
 * Everything here is UI state: what the steps collect, and the validation that gives
 * immediate feedback. The real validation happens in Rust, which builds the profile and
 * refuses what it cannot use — this only exists so a person is told before they submit.
 *
 * The form used to carry a `config`, an `envelope` and an `authorizedFingerprint`, because
 * authorization was a step whose result could go stale against answers changed afterwards.
 * Creating a hub is one call now, over answers that are already final, so none of that
 * exists any more.
 */
export type HubForm = {
  /** Docker answered on all three counts; the first step refuses to pass without it. */
  dockerOk: boolean;
  path: string;
  name: string;
  folderOk: boolean;
  coordServer: string;
  identifier: string;
  description: string;
  rekuestServer: string;
  services: ServiceId[];
  httpPort: number;
  httpsPort: number;
  ssl: boolean;
  domain: string;
  globalDescription: string;
  globalAdmin: string;
  globalAdminPassword: string;
  hosts: AdvertisedHost[];
  meshMode: MeshMode;
  /** Only for `meshMode: "manual"`; the coordination server supplies the other one. */
  meshAuthKey: string;
  meshCoordUrl: string;
  /**
   * What was asked of individual services under the gear on the services step: whether
   * each runs from a checkout of its source rather than its published image, and on
   * which branch. Only the services somebody touched are in here.
   *
   * Source mode needs git, so every entry is cleared if a probe comes back without it.
   */
  serviceOptions: Partial<Record<ServiceId, ServiceOverride>>;
};

/** One service's answers. Empty strings rather than nulls, as the inputs produce them. */
export type ServiceOverride = {
  fromSource: boolean;
  branch: string;
  /**
   * Django's debug mode for this one service. The only setting here that needed nothing
   * from the generator — it was already being written, and only the question was missing.
   */
  debug: boolean;
  /**
   * Alpaka only. `""` means nothing was asked and the profile keeps saying what it says
   * today: a provider that nothing starts.
   */
  ollama: "" | "local" | "remote";
  /** Alpaka only, and only for `ollama: "remote"`. */
  ollamaUrl: string;
  /**
   * Kabinet only: one repository per line, as the textarea gives them. Empty leaves the
   * seeded pair alone.
   */
  repositories: string;
};

/** What an untouched service carries, so the gear never reads `undefined`. */
export const emptyOverride: ServiceOverride = {
  fromSource: false,
  branch: "",
  debug: false,
  ollama: "",
  ollamaUrl: "",
  repositories: "",
};

/** The textarea's lines, trimmed and without the blanks. */
export const repositoryList = (text: string): string[] =>
  text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);

/** A bare host means https; anything with a scheme is taken as given. */
export const baseUrl = (server: string): string => {
  const trimmed = server.trim();
  const withScheme = trimmed.includes("://") ? trimmed : `https://${trimmed}`;
  return withScheme.replace(/\/+$/, "");
};

/**
 * A coordination server address, as the picker lets one be given.
 *
 * Either a bare host or a full URL: insisting on the bare-host shape would reject
 * `http://localhost:8000`, which is what somebody running a coordination server on this
 * machine will paste.
 */
export const coordinationServerSchema = z
  .string()
  .trim()
  .min(1, "Choose a coordination server")
  .refine((value) => !/\s/.test(value), "An address cannot contain spaces")
  .refine((value) => {
    try {
      return new URL(baseUrl(value)).hostname.length > 0;
    } catch {
      return false;
    }
  }, "That does not look like a server address");

/** The last path segment, splitting on either separator. */
export const basename = (path: string): string => {
  const trimmed = path.replace(/[\\/]+$/, "");
  const parts = trimmed.split(/[\\/]/);
  return parts[parts.length - 1] ?? "";
};

/**
 * One service's gear answers, or `undefined` when it was left alone.
 *
 * Absent is not the same as all-false: the core reads a missing service as "take the
 * default", and sending an empty answer for every service would be noise in the profile.
 */
export const serviceAnswer = (asked: ServiceOverride): ServiceOptions | undefined => {
  const repositories = repositoryList(asked.repositories ?? "");
  const url = asked.ollamaUrl?.trim() ?? "";

  const ollama =
    asked.ollama === "local"
      ? { run_locally: true, url: null }
      : asked.ollama === "remote" && url
        ? { run_locally: false, url }
        : null;

  const answer: ServiceOptions = {
    from_source: asked.fromSource,
    branch: asked.fromSource ? asked.branch?.trim() || null : null,
    debug: asked.debug,
    ollama,
    repositories: repositories.length > 0 ? repositories : null,
  };

  const untouched =
    !answer.from_source && !answer.debug && !answer.ollama && !answer.repositories;
  return untouched ? undefined : answer;
};
