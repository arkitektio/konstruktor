import { describe, expect, it } from "vitest";
import { advance, newProgressState } from "../compose-progress";

const line = (text: string) => ({ line: text, stderr: true });

describe("compose progress", () => {
  it("counts a container once and finishes it on the past tense", () => {
    const state = newProgressState();
    expect(advance(state, line(" Container hub-db-1  Starting"), "hub")).toMatchObject({
      fraction: 0,
      step: "db · Starting",
      total: 1,
    });
    expect(advance(state, line(" Container hub-db-1  Started"), "hub")).toMatchObject({
      fraction: 1,
      step: "db · Started",
      done: 1,
    });
  });

  it("keeps the fraction across subjects and ignores chatter", () => {
    const state = newProgressState();
    advance(state, line(" Container hub-db-1  Started"), "hub");
    advance(state, line(" Container hub-rekuest-1  Starting"), "hub");
    const chatter = advance(state, line("[+] Running 2/2"), "hub");
    expect(chatter.fraction).toBe(0.5);
    expect(chatter.step).toBeUndefined();
    expect(advance(state, line(" Image jhnnsrs/rekuest:next  Pulled")).step).toBe(
      "rekuest:next · Pulled"
    );
  });

  it("has no fraction before compose names anything", () => {
    expect(advance(newProgressState(), line("some preamble")).fraction).toBeUndefined();
  });
});
