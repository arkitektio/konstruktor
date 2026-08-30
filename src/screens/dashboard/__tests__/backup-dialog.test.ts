import { describe, expect, it } from "vitest";
import { reduceBackup } from "../BackupDialog";

describe("reduceBackup", () => {
  it("marks the previous step done when the next one starts", () => {
    let steps = reduceBackup({}, { event: "step", step: "deployment", title: "x" });
    steps = reduceBackup(steps, { event: "line", step: "deployment", line: "hub_config.yaml", stderr: false });
    expect(steps.deployment).toEqual({ status: "running", last: "hub_config.yaml" });

    steps = reduceBackup(steps, { event: "step", step: "dump", title: "y" });
    expect(steps.deployment.status).toBe("done");
    expect(steps.dump.status).toBe("running");
  });

  it("keeps a skipped step's reason", () => {
    const steps = reduceBackup({}, { event: "skipped", step: "minio", reason: "never started" });
    expect(steps.minio).toEqual({ status: "skipped", last: "never started" });
  });
});
