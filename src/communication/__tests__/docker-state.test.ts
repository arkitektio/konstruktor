import { describe, expect, it } from "vitest";
import { DockerProbe, dockerState } from "../communication-context";

/**
 * The probe answers three questions, and each "no" has its own remedy. Collapsing them
 * into one boolean is what used to send somebody whose Docker was merely stopped to a
 * download page.
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
  ...over,
});

describe("what the wizard makes of a docker probe", () => {
  it("is still checking before the first probe comes back", () => {
    expect(dockerState(null)).toBe("checking");
  });

  it("is ready only when all three answered", () => {
    expect(dockerState(probe())).toBe("ready");
  });

  it("asks for an install when the binary is not there", () => {
    expect(dockerState(probe({ cli: false, cli_version: null }))).toBe("missing");
  });

  it("names compose when the CLI is there without its plugin", () => {
    expect(dockerState(probe({ compose: false, compose_version: null }))).toBe(
      "no-compose"
    );
  });

  it("asks for the daemon to be started, not for a download", () => {
    expect(
      dockerState(probe({ daemon: false, api_version: null, error: "no socket" }))
    ).toBe("no-daemon");
  });

  it("reports the missing binary first — a stopped daemon is not the point then", () => {
    expect(dockerState(probe({ cli: false, compose: false, daemon: false }))).toBe(
      "missing"
    );
  });
});
