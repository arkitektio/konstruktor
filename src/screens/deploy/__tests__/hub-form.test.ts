import { describe, expect, it } from "vitest";
import {
  basename,
  baseUrl,
  coordinationServerSchema,
  emptyOverride,
  serviceAnswer,
  type ServiceOverride,
} from "../hub-form";

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

describe("one service's gear answers", () => {
  const asked = (over: Partial<ServiceOverride> = {}): ServiceOverride => ({
    ...emptyOverride,
    ...over,
  });

  /**
   * The core reads a missing service as "take the default", so an untouched one has to
   * stay out of the profile entirely rather than go in as a row of falses.
   */
  it("says nothing about a service nobody touched", () => {
    expect(serviceAnswer(asked())).toBeUndefined();
  });

  /**
   * The regression this guards: the mapping used to keep only services with `fromSource`
   * set, so every other answer the gear collects was silently dropped on the way out.
   */
  it("keeps an answer that is not about source at all", () => {
    expect(serviceAnswer(asked({ debug: true }))).toEqual({
      from_source: false,
      branch: null,
      debug: true,
      ollama: null,
      repositories: null,
    });
  });

  it("sends a branch only for a service actually running from source", () => {
    expect(serviceAnswer(asked({ fromSource: true, branch: " main " })!)?.branch).toBe(
      "main"
    );
    // A branch left behind after source mode was switched off is not an instruction.
    expect(serviceAnswer(asked({ fromSource: false, branch: "main" }))).toBeUndefined();
  });

  it("turns the two Ollama answers into what the core expects", () => {
    expect(serviceAnswer(asked({ ollama: "local" }))?.ollama).toEqual({
      run_locally: true,
      url: null,
    });
    expect(
      serviceAnswer(asked({ ollama: "remote", ollamaUrl: " gpu-box.lab:11434 " }))
        ?.ollama
    ).toEqual({ run_locally: false, url: "gpu-box.lab:11434" });
  });

  /**
   * "Use one that already exists" with nothing typed is not an answer. The wizard's
   * schema holds the step for this, and the mapping refuses to invent a provider.
   */
  it("refuses to invent a provider from an empty address", () => {
    expect(serviceAnswer(asked({ ollama: "remote", ollamaUrl: "   " }))).toBeUndefined();
  });

  it("reads the repository box as one entry per line", () => {
    expect(
      serviceAnswer(asked({ repositories: " a/one:main \n\n  b/two:dev \n" }))
        ?.repositories
    ).toEqual(["a/one:main", "b/two:dev"]);
    expect(serviceAnswer(asked({ repositories: "  \n \n" }))).toBeUndefined();
  });
});
