import type { AdvertisedHost, HostCandidate, ReachPreset } from "../../api";
import type { ReachChoice } from "./HostPicker";

/**
 * Turning a preset into a selection, and a selection back into a preset.
 *
 * Which categories each preset takes is decided in `konstruktor-core::hosts` and arrives
 * with the candidates, so the wizard and `konstruktor create --reach` cannot drift apart.
 * What lives here is only the bookkeeping either side of that: matching values up with
 * their candidates, and working out which preset — if any — a seeded selection came from.
 */

/** The preset a fresh, unseeded picker opens on. */
export const DEFAULT_REACH = "this-network";

export const selectionFor = (
  candidates: HostCandidate[],
  preset: ReachPreset
): AdvertisedHost[] =>
  candidates
    .filter((candidate) => preset.values.includes(candidate.value))
    .map((candidate) => ({ host: candidate.value, kind: candidate.kind }));

/**
 * The preset to open on when nothing was chosen before.
 *
 * "This network" unless this machine has nothing that answers to it — a host with only
 * loopback, which is a laptop with the wifi off — in which case the honest offer is
 * "local only" rather than an empty selection the step will refuse to pass.
 */
export const defaultPreset = (presets: ReachPreset[]): ReachPreset | undefined => {
  const preferred = presets.find(
    (preset) => preset.id === DEFAULT_REACH && preset.values.length > 0
  );
  return preferred ?? presets.find((preset) => preset.values.length > 0);
};

/**
 * The widest preset with anything in it.
 *
 * For a hub authorized before konstruktor recorded what it advertised: its previous set
 * is unknowable, and the old code recommended every real address including public ones,
 * so the narrow default would quietly drop a public alias on the next authorization.
 * Erring wide keeps what was probably there; the screen says why.
 */
export const widestPreset = (presets: ReachPreset[]): ReachPreset | undefined =>
  [...presets].reverse().find((preset) => preset.values.length > 0);

/**
 * Which preset a selection corresponds to, or `"custom"`.
 *
 * Checked narrowest-first, because the presets nest: on a machine with no public address
 * "public" and "this network" select exactly the same things, and calling that "this
 * network" is the less surprising of the two.
 */
export const reachFor = (
  presets: ReachPreset[],
  selected: AdvertisedHost[]
): ReachChoice => {
  const chosen = selected.map((host) => host.host);
  const match = presets.find(
    (preset) =>
      preset.values.length === chosen.length &&
      preset.values.every((value) => chosen.includes(value))
  );
  return match?.id ?? "custom";
};

/** Adds or removes one host, keeping the kind the core gave it. */
export const toggleHost = (
  selected: AdvertisedHost[],
  candidates: HostCandidate[],
  value: string
): AdvertisedHost[] => {
  if (selected.some((host) => host.host === value)) {
    return selected.filter((host) => host.host !== value);
  }
  const candidate = candidates.find((c) => c.value === value);
  // A host with no candidate cannot be re-added — it was only removable because it was
  // already in the selection, carrying the kind it was authorized with.
  return candidate
    ? [...selected, { host: candidate.value, kind: candidate.kind }]
    : selected;
};
