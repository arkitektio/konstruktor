import { describe, expect, it } from "vitest";
import { basename, baseUrl, coordinationServerSchema } from "../hub-form";

/**
 * What is left of the form module after generation moved to Rust: the address rule the
 * coordination picker validates against, and the path helper the folder step labels with.
 *
 * The identifier suggestion, the manifest fingerprint and the authorization staleness
 * check all used to live here. The first is in the core now; the other two stopped
 * existing when authorization became part of one `create_hub` call rather than a wizard
 * step whose result could go stale.
 */

describe("the coordination server address", () => {
  const accepts = (value: string) =>
    coordinationServerSchema.safeParse(value).success;

  it("takes a bare host", () => {
    expect(accepts("go.arkitekt.live")).toBe(true);
  });

  it("takes a pasted URL, which is what the picker gets from a clipboard", () => {
    expect(accepts("https://go.arkitekt.live")).toBe(true);
    expect(accepts("https://go.arkitekt.live/")).toBe(true);
  });

  it("takes a host with a port, so a server on this machine can be used", () => {
    expect(accepts("http://localhost:8000")).toBe(true);
    expect(accepts("localhost:8000")).toBe(true);
  });

  it("refuses what cannot be an address", () => {
    expect(accepts("")).toBe(false);
    expect(accepts("   ")).toBe(false);
    expect(accepts("two words")).toBe(false);
  });
});

describe("normalising a server address", () => {
  it("reaches a bare host over https", () => {
    expect(baseUrl("go.arkitekt.live")).toBe("https://go.arkitekt.live");
    expect(baseUrl("  go.arkitekt.live  ")).toBe("https://go.arkitekt.live");
  });

  it("leaves a scheme alone, so a local server stays reachable", () => {
    expect(baseUrl("http://localhost:8000")).toBe("http://localhost:8000");
    expect(baseUrl("https://go.arkitekt.live/")).toBe("https://go.arkitekt.live");
  });
});

describe("naming a folder", () => {
  it("takes a path apart on either separator", () => {
    expect(basename("/home/someone/MyHub")).toBe("MyHub");
    expect(basename("/home/someone/MyHub/")).toBe("MyHub");
    // A Windows path may well be shown on Linux.
    expect(basename("C:\\Users\\Someone\\MyHub")).toBe("MyHub");
  });
});
