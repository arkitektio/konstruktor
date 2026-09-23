/**
 * Pages on arkitekt.live the app points at. Kept in one place so a moved page is one
 * edit, not a search.
 */
export const DOCS = {
  /** What the mesh is, how a hub joins it, and what mesh-only gives up. */
  mesh: "https://arkitekt.live/docs/konstruktor/mesh",
  /** Why a hub's sidecar does not interfere with a Tailscale the machine already runs. */
  meshWithTailscale: "https://arkitekt.live/docs/konstruktor/mesh#existing-tailscale",
} as const;
