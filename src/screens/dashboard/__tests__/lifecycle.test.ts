import { describe, expect, it } from "vitest";

import type { Container, HubStatus, ImageState } from "../../../api";
import {
  isInitContainer,
  runSummary,
  serviceUpdates,
  stages,
  updateSummary,
} from "../lifecycle";

const container = (over: Partial<Container>): Container => ({
  id: "c1",
  names: ["/c1"],
  image: "jhnnsrs/rekuest:next",
  image_id: "sha256:aaa",
  status: "Up 2 hours",
  state: "running",
  service: "rekuest",
  ...over,
});

const image = (over: Partial<ImageState>): ImageState => ({
  image: "jhnnsrs/rekuest:next",
  service: "rekuest",
  present: true,
  image_id: "sha256:aaa",
  created: "2026-08-01T10:00:00Z",
  ...over,
});

describe("runSummary", () => {
  /** The distinction the buttons hang off: nothing to restart versus something to start. */
  it("tells an empty project from one whose containers merely exited", () => {
    expect(runSummary([]).state).toBe("never");
    expect(runSummary([container({ state: "exited" })]).state).toBe("stopped");
  });

  it("counts a half-up stack as partial", () => {
    const summary = runSummary([
      container({ id: "a", state: "running" }),
      container({ id: "b", state: "exited" }),
    ]);
    expect(summary).toEqual({ state: "partial", running: 1, total: 2 });
  });

  /**
   * Infrastructure counts. The old page grouped every container but rendered only the
   * profile's services, so a hub with a dead database looked entirely healthy.
   */
  it("counts infrastructure containers, not just services", () => {
    const summary = runSummary([
      container({ id: "a", service: "rekuest", state: "running" }),
      container({ id: "b", service: "db", state: "exited" }),
    ]);
    expect(summary.state).toBe("partial");
    expect(summary.total).toBe(2);
  });
});

describe("init containers", () => {
  it("knows the run-once ones by their compose service", () => {
    expect(isInitContainer(container({ service: "minio_init" }))).toBe(true);
    expect(isInitContainer(container({ service: "db" }))).toBe(false);
    expect(isInitContainer(container({ service: undefined }))).toBe(false);
  });

  /** The whole point: an init container that exited is not a stack that half fell over. */
  it("leaves them out of the run summary", () => {
    const summary = runSummary([
      container({ id: "a", service: "rekuest", state: "running" }),
      container({ id: "b", service: "minio_init", state: "exited" }),
    ]);
    expect(summary).toEqual({ state: "running", running: 1, total: 1 });
  });

  it("still reports an empty project as never started", () => {
    expect(
      runSummary([container({ service: "minio_init", state: "exited" })]).state
    ).toBe("never");
  });
});

describe("serviceUpdates", () => {
  it("calls a container running the tag's current image up to date", () => {
    const [update] = serviceUpdates([image({})], [container({})]);
    expect(update.state).toBe("current");
    expect(update.tag).toBe("next");
  });

  /**
   * The whole point of comparing ids: both sides still say `next`, and only the id shows
   * that a pull has moved the tag out from under the running container.
   */
  it("spots an image pulled after the container started", () => {
    const [update] = serviceUpdates(
      [image({ image_id: "sha256:bbb" })],
      [container({ image_id: "sha256:aaa" })]
    );
    expect(update.state).toBe("pulled");
  });

  it("separates an image nobody has pulled from one that is merely not running", () => {
    const never = serviceUpdates([image({ present: false, image_id: null })], []);
    expect(never[0].state).toBe("missing");

    const notRunning = serviceUpdates([image({})], []);
    expect(notRunning[0].state).toBe("unknown");
  });

  it("reads the tag off a registry reference that carries a port", () => {
    const [update] = serviceUpdates(
      [image({ image: "registry.local:5000/jhnnsrs/mikro:dev", service: "mikro" })],
      []
    );
    expect(update.tag).toBe("dev");
  });

  it("groups the pending ones by what they need", () => {
    const summary = updateSummary(
      serviceUpdates(
        [
          image({ service: "rekuest", image_id: "sha256:bbb" }),
          image({ service: "mikro", present: false, image_id: null }),
          image({ service: "fluss" }),
        ],
        [
          container({ id: "a", service: "rekuest", image_id: "sha256:aaa" }),
          container({ id: "c", service: "fluss", image_id: "sha256:aaa" }),
        ]
      )
    );
    expect(summary.pulled.map((u) => u.service)).toEqual(["rekuest"]);
    expect(summary.missing.map((u) => u.service)).toEqual(["mikro"]);
  });
});

describe("stages", () => {
  const status = (over: Partial<HubStatus>): HubStatus =>
    ({
      profile: { config: { coord_server: "https://coord.example" } },
      authorized: true,
      identifier: "my-hub",
      authorized_at: "2026-08-01T10:00:00Z",
      services: [],
      channel: { tag: "next", tags: ["next"] },
      ...over,
    }) as HubStatus;

  it("flags a hub that was never authorized", () => {
    const rail = stages(
      { createdAt: "2026-07-01T10:00:00Z" },
      status({ authorized: false, authorized_at: null, identifier: null }),
      runSummary([])
    );
    expect(rail.find((s) => s.key === "authorized")?.state).toBe("attention");
  });

  /**
   * Re-authorizing can move the JWKS URL the services verify tokens against, so configs
   * written before the last authorization no longer describe the hub the coordination
   * server knows about.
   */
  it("flags configs written before the last authorization", () => {
    const rail = stages(
      {
        createdAt: "2026-07-01T10:00:00Z",
        lastGeneratedAt: "2026-07-15T10:00:00Z",
      },
      status({ authorized_at: "2026-08-01T10:00:00Z" }),
      runSummary([])
    );
    expect(rail.find((s) => s.key === "generated")?.state).toBe("attention");
  });

  it("leaves configs alone when they were written after the authorization", () => {
    const rail = stages(
      {
        createdAt: "2026-07-01T10:00:00Z",
        lastGeneratedAt: "2026-08-02T10:00:00Z",
      },
      status({}),
      runSummary([])
    );
    expect(rail.find((s) => s.key === "generated")?.state).toBe("done");
  });

  it("reports an empty stack without claiming it never ran", () => {
    const rail = stages(
      { createdAt: "2026-07-01T10:00:00Z" },
      status({}),
      runSummary([])
    );
    const started = rail.find((s) => s.key === "started");
    expect(started?.state).toBe("waiting");
    expect(started?.detail).toBe("No containers — nothing is up right now");
  });

  it("survives a status that could not be read at all", () => {
    const rail = stages({ createdAt: "2026-07-01T10:00:00Z" }, undefined, runSummary([]));
    expect(rail).toHaveLength(4);
    expect(rail.find((s) => s.key === "authorized")?.state).toBe("attention");
  });
});
