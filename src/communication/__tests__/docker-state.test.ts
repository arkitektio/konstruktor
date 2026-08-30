import { describe, expect, it } from "vitest";
import { DockerProbe, dockerState } from "../communication-context";

/**
 * The verdict is the core's. The probe arrives with its `state` already decided — by the
 * same code `konstruktor doctor` runs — and this side adds only "checking", for before
 * the first probe has come back. It must never re-derive the verdict from the booleans:
 * that is how the two front ends used to drift.
 */

const probe = (over: Partial<DockerProbe> = {}): DockerProbe => ({
  cli: true,
  cli_version: "27.3.1",
  compose: true,
  compose_version: "2.29.7",
  daemon: true,
  api_version: "1.47",
  memory: 16_000_000_000,
  error: null,
  engine: "docker",
  brand: "colima",
  platform: "macos",
  state: "ready",
  remedies: [],
  ...over,
});

describe("what the app makes of a docker probe", () => {
  it("is still checking before the first probe comes back", () => {
    expect(dockerState(null)).toBe("checking");
  });

  it("reads the verdict the core decided", () => {
    expect(dockerState(probe())).toBe("ready");
    expect(dockerState(probe({ state: "missing" }))).toBe("missing");
    expect(dockerState(probe({ state: "no-compose" }))).toBe("no-compose");
    expect(dockerState(probe({ state: "no-daemon" }))).toBe("no-daemon");
    expect(dockerState(probe({ state: "too-old" }))).toBe("too-old");
  });

  it("does not second-guess the core from the booleans", () => {
    // The booleans say "missing"; the core said "no-daemon". The core wins — it is the
    // only place the priority of the failures is written down.
    expect(dockerState(probe({ cli: false, state: "no-daemon" }))).toBe("no-daemon");
  });
});
