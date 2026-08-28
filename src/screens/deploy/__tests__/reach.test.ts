import { describe, expect, it } from "vitest";

import type { AdvertisedHost, HostCandidate, ReachPreset } from "../../../api";
import {
  defaultPreset,
  reachFor,
  selectionFor,
  toggleHost,
  widestPreset,
} from "../reach";

const candidate = (
  value: string,
  kind: HostCandidate["kind"],
  usable = true
): HostCandidate => ({
  value,
  kind,
  interface: "eth0",
  recommended: usable,
  usable,
  unusable_reason: usable ? null : "virtual-interface",
  summary: "",
});

const preset = (
  id: ReachPreset["id"],
  values: string[]
): ReachPreset => ({ id, label: id, description: "", values });

const CANDIDATES = [
  candidate("140.78.80.150", "public"),
  candidate("10.0.0.4", "private"),
  candidate("100.116.108.106", "mesh"),
  candidate("127.0.0.1", "loopback"),
  candidate("172.17.0.1", "virtual", false),
];

const PRESETS = [
  preset("local-only", ["127.0.0.1"]),
  preset("this-network", ["127.0.0.1", "10.0.0.4", "100.116.108.106"]),
  preset("public", ["127.0.0.1", "10.0.0.4", "100.116.108.106", "140.78.80.150"]),
];

describe("selectionFor", () => {
  it("carries the kind the core gave each address", () => {
    const selection = selectionFor(CANDIDATES, PRESETS[1]);
    expect(selection).toEqual([
      { host: "10.0.0.4", kind: "private" },
      { host: "100.116.108.106", kind: "mesh" },
      { host: "127.0.0.1", kind: "loopback" },
    ]);
  });

  it("ignores values with no candidate", () => {
    expect(selectionFor(CANDIDATES, preset("public", ["nope"]))).toEqual([]);
  });
});

describe("defaultPreset", () => {
  it("opens on this-network", () => {
    expect(defaultPreset(PRESETS)?.id).toBe("this-network");
  });

  /** A laptop with the wifi off: offering an empty selection the step will refuse is worse. */
  it("falls back when nothing on this machine reaches the network", () => {
    const presets = [
      preset("local-only", ["127.0.0.1"]),
      preset("this-network", []),
      preset("public", []),
    ];
    expect(defaultPreset(presets)?.id).toBe("local-only");
  });

  it("has nothing to offer when no preset has anything", () => {
    expect(defaultPreset([preset("this-network", [])])).toBeUndefined();
  });
});

describe("widestPreset", () => {
  it("picks the widest preset that has anything in it", () => {
    expect(widestPreset(PRESETS)?.id).toBe("public");
  });

  /** A machine with no public address: "public" is empty, so the next one down wins. */
  it("skips empty presets", () => {
    const presets = [
      preset("local-only", ["127.0.0.1"]),
      preset("this-network", ["127.0.0.1", "10.0.0.4"]),
      preset("public", []),
    ];
    expect(widestPreset(presets)?.id).toBe("this-network");
  });

  it("has nothing to offer when every preset is empty", () => {
    expect(widestPreset([preset("public", [])])).toBeUndefined();
  });
});

describe("reachFor", () => {
  it("names the preset a selection came from", () => {
    const selection = selectionFor(CANDIDATES, PRESETS[1]);
    expect(reachFor(PRESETS, selection)).toBe("this-network");
  });

  /**
   * The presets nest, so on a machine with no public address "public" and "this network"
   * select exactly the same things. Narrowest-first makes that read as the narrower one.
   */
  it("prefers the narrower preset when two select the same addresses", () => {
    const presets = [
      preset("local-only", ["127.0.0.1"]),
      preset("this-network", ["127.0.0.1", "10.0.0.4"]),
      preset("public", ["127.0.0.1", "10.0.0.4"]),
    ];
    const selection: AdvertisedHost[] = [
      { host: "127.0.0.1", kind: "loopback" },
      { host: "10.0.0.4", kind: "private" },
    ];
    expect(reachFor(presets, selection)).toBe("this-network");
  });

  it("calls a hand-made selection custom", () => {
    const selection: AdvertisedHost[] = [{ host: "10.0.0.4", kind: "private" }];
    expect(reachFor(PRESETS, selection)).toBe("custom");
  });

  it("calls an empty selection custom rather than a preset", () => {
    expect(reachFor(PRESETS, [])).toBe("custom");
  });
});

describe("toggleHost", () => {
  it("adds and removes, keeping the kind", () => {
    const added = toggleHost([], CANDIDATES, "100.116.108.106");
    expect(added).toEqual([{ host: "100.116.108.106", kind: "mesh" }]);
    expect(toggleHost(added, CANDIDATES, "100.116.108.106")).toEqual([]);
  });

  /**
   * The case that used to be impossible: a hub that moved networks still advertises a
   * host this machine cannot find, and it has to be removable or it is stuck forever.
   */
  it("removes a host that no longer has a candidate", () => {
    const stale: AdvertisedHost[] = [{ host: "10.9.9.9", kind: "private" }];
    expect(toggleHost(stale, CANDIDATES, "10.9.9.9")).toEqual([]);
  });

  it("will not invent a host it knows nothing about", () => {
    expect(toggleHost([], CANDIDATES, "10.9.9.9")).toEqual([]);
  });
});
