import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AdvertisedHost, HostCandidate, ReachPreset } from "../../../api";
import { HostPicker } from "../HostPicker";

afterEach(() => cleanup());

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
  summary: `what ${value} is`,
});

const CANDIDATES = [
  candidate("140.78.80.150", "public"),
  candidate("10.0.0.4", "private"),
  candidate("100.116.108.106", "mesh"),
  candidate("100.70.0.9", "other-mesh"),
  candidate("172.17.0.1", "virtual", false),
];

const PRESETS: ReachPreset[] = [
  { id: "local-only", label: "Local only", description: "", values: [] },
  {
    id: "this-network",
    label: "This network",
    description: "",
    values: ["10.0.0.4", "100.116.108.106"],
  },
  {
    id: "public",
    label: "Public",
    description: "",
    values: ["10.0.0.4", "100.116.108.106", "140.78.80.150"],
  },
];

const setup = (props: Partial<Parameters<typeof HostPicker>[0]> = {}) => {
  const onToggle = vi.fn();
  const onReachChange = vi.fn();
  render(
    <HostPicker
      candidates={CANDIDATES}
      presets={PRESETS}
      selected={[]}
      reach="this-network"
      onReachChange={onReachChange}
      onToggle={onToggle}
      {...props}
    />
  );
  // The preset row and the detail list deliberately share wording ("Public" is both a
  // reach and a category), so tests scope their queries to one or the other.
  const presetRow = () => screen.getByText("Reach").closest("div")!.parentElement!;
  return { onToggle, onReachChange, presetRow };
};

describe("HostPicker", () => {
  it("groups addresses under headings rather than one flat list", () => {
    setup();
    expect(screen.getByText("Local network")).toBeDefined();
    expect(screen.getByText("Mesh")).toBeDefined();
    // "Public" is both a preset and a group heading — two of them is the correct count.
    expect(screen.getAllByText("Public")).toHaveLength(2);
    // Nothing on this machine is loopback, so that group is not drawn at all.
    expect(screen.queryByText("This machine")).toBeNull();
  });

  /**
   * A machine on a personal tailnet as well as the hub's: both are `100.x`, and only one
   * of them is reachable by anybody the coordination server knows about.
   */
  it("separates another tailnet from this hub's mesh", () => {
    setup();
    expect(screen.getByText("Mesh")).toBeDefined();
    expect(screen.getByText("Other tailscales")).toBeDefined();
    expect(screen.getByText("100.70.0.9")).toBeDefined();
    // Visible and tickable, not tucked away with the bridges.
    expect(screen.getByText("other tailnet")).toBeDefined();
  });

  it("hides what is not worth advertising until asked", () => {
    setup();
    expect(screen.queryByText("172.17.0.1")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /Show 1 address that/i }));
    expect(screen.getByText("172.17.0.1")).toBeDefined();
  });

  it("disables a preset with nothing to select", () => {
    const { onReachChange, presetRow } = setup();
    fireEvent.click(within(presetRow()).getByText("Local only"));
    expect(onReachChange).not.toHaveBeenCalled();

    fireEvent.click(within(presetRow()).getByText("Public"));
    expect(onReachChange).toHaveBeenCalledWith(PRESETS[2]);
  });

  it("says so when the selection matches no preset", () => {
    setup({ reach: "custom" });
    expect(screen.getByText("custom")).toBeDefined();
  });

  /**
   * A hub that moved networks still advertises hosts this machine cannot find. They used
   * to vanish from the picker while staying in the manifest — visible and untickable is
   * the only way they can be removed.
   */
  it("keeps a selected host that is no longer discoverable", () => {
    const stale: AdvertisedHost[] = [{ host: "10.9.9.9", kind: "private" }];
    const { onToggle } = setup({ selected: stale });

    expect(screen.getByText("Advertised before, not found here now")).toBeDefined();
    fireEvent.click(screen.getByText("10.9.9.9"));
    expect(onToggle).toHaveBeenCalledWith("10.9.9.9");
  });

  /**
   * The two facts are different questions. Matching this machine's egress address says
   * nothing about whether a port is open, so it must never render as though it does.
   */
  it("keeps egress identity and an actual probe apart", () => {
    setup({
      reachability: { "140.78.80.150": { egress: true } },
      canProbe: false,
    });

    expect(screen.getByText("the internet sees you at this address")).toBeDefined();
    expect(screen.queryByText("answered from outside")).toBeNull();
    expect(
      screen.getAllByText(/cannot check from outside until the hub is running/i).length
    ).toBeGreaterThan(0);
  });

  it("reports a probe that came back", () => {
    setup({
      reachability: {
        "140.78.80.150": { probe: { result: "reachable", status: 200 } },
        "10.0.0.4": { probe: { result: "unreachable", reason: "timed out" } },
      },
      canProbe: true,
    });

    expect(screen.getByText("answered from outside")).toBeDefined();
    expect(screen.getByText("nothing answered from outside")).toBeDefined();
  });

  it("shows its own loading and empty states", () => {
    const { unmount } = render(
      <HostPicker
        candidates={[]}
        presets={[]}
        selected={[]}
        reach="custom"
        onReachChange={vi.fn()}
        onToggle={vi.fn()}
        loading
      />
    );
    expect(screen.getByText("Looking at this machine…")).toBeDefined();
    unmount();

    render(
      <HostPicker
        candidates={[]}
        presets={[]}
        selected={[]}
        reach="custom"
        onReachChange={vi.fn()}
        onToggle={vi.fn()}
      />
    );
    expect(screen.getByText("No addresses were found on this machine.")).toBeDefined();
  });
});
