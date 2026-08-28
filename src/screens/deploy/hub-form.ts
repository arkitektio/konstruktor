import { z } from "zod";
import type { AdvertisedHost, MeshMode, ServiceId } from "../../api";

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
};

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
