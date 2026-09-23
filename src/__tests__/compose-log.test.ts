import { describe, expect, it } from "vitest";
import { appendLine, errorLines, type LogEntry } from "../compose-log";

const feed = (lines: string[]) =>
  lines.reduce((entries, line) => appendLine(entries, { line, stderr: true }), [] as LogEntry[]);

describe("compose log", () => {
  it("keeps one line per image and per layer, updated in place", () => {
    const log = feed([
      " Image caddy:2.11.4 Pulling ",
      " 034d6572bf28 Pulling fs layer 0B",
      " 034d6572bf28 Downloading 1.049MB",
      " 034d6572bf28 Download complete 0B",
      " Image caddy:2.11.4 Pulled ",
    ]);
    expect(log.map((e) => e.text)).toEqual([
      "Image caddy:2.11.4 Pulled",
      "034d6572bf28 Download complete 0B",
    ]);
  });

  it("marks the lines that say what went wrong", () => {
    const log = feed([
      " Image doesnotexist/nope:latest Pulling ",
      " Image doesnotexist/nope:latest Error pull access denied for doesnotexist/nope",
    ]);
    expect(log).toHaveLength(1);
    expect(log[0].error).toBe(true);
  });

  it("leads a failure with each error once", () => {
    const output = [
      " Image busybox:1.36 Pulling ",
      " Image doesnotexist/nope:latest Error pull access denied for doesnotexist/nope, repository does not exist",
      " Image busybox:1.36 Interrupted ",
      "Error response from daemon: pull access denied for doesnotexist/nope, repository does not exist",
    ].join("\n");
    expect(errorLines(output)).toEqual([
      "Image doesnotexist/nope:latest Error pull access denied for doesnotexist/nope, repository does not exist",
    ]);
  });
});
