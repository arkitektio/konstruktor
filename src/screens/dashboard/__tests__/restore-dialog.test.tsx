import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MemoryRouter } from "react-router-dom";

import type { BackupManifest, DeploymentRecord, RestorePlan } from "../../../api";
import { RestoreDialog, reduceRestore } from "../RestoreDialog";

const MANIFEST: BackupManifest = {
  format: 1,
  konstruktor_version: "test",
  taken_at: 0,
  storage: "docker-volumes",
  hub: { identifier: "lab-hub", coord_server: "go.arkitekt.live", project: "myhub", path: "/x" },
  services: [
    { id: "rekuest", host: "rekuest", image: "jhnnsrs/rekuest:next", image_id: "a", repo_digests: [], db: "rekuest" },
    { id: "mikro", host: "mikro", image: "jhnnsrs/mikro:next", image_id: "b", repo_digests: [], db: "mikro" },
  ],
  infrastructure: [{ service: "db", image: "jhnnsrs/daten:dev", image_id: null }],
  postgres: { user: "u", server_version: null },
  contents: { dumped: true, postgres_copied: true, minio_copied: true, deployment_files: [], warnings: [] },
};

let planResult: RestorePlan;
const readBackupManifest = vi.fn(async () => MANIFEST);
const restorePlan = vi.fn(async () => planResult);

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: async () => "/backups/one" }));
vi.mock("../../../api", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  readBackupManifest: () => readBackupManifest(),
  restorePlan: () => restorePlan(),
}));

const DEPLOYMENT: DeploymentRecord = {
  id: "abc",
  name: "MyHub",
  path: "/home/someone/MyHub",
  kind: "hub",
} as DeploymentRecord;

const basePlan = (): RestorePlan => ({
  manifest: MANIFEST,
  same_hub: true,
  target_identifier: "lab-hub",
  target_storage: "docker-volumes",
  services: [
    { id: "rekuest", host: "rekuest", backup_image: "jhnnsrs/rekuest:next", backup_image_id: "a", deployed_image: "jhnnsrs/rekuest:next", deployed_image_id: "a", verdict: "same" },
    { id: "mikro", host: "mikro", backup_image: "jhnnsrs/mikro:next", backup_image_id: "b", deployed_image: "jhnnsrs/mikro:next", deployed_image_id: "c", verdict: "different-build" },
  ],
  extra_in_target: [],
  db: { service: "db", backup_image: "jhnnsrs/daten:dev", deployed_image: "jhnnsrs/daten:dev", verdict: "same" },
  target_postgres_version: null,
  available: { dump: true, postgres_raw: true, minio: true },
  blocking: [],
  warnings: ["mikro is on the same tag but a different build"],
});

afterEach(cleanup);

const mount = () =>
  render(
    <MemoryRouter>
      <RestoreDialog open deployment={DEPLOYMENT} onOpenChange={() => undefined} />
    </MemoryRouter>
  );

describe("RestoreDialog", () => {
  it("shows a mismatch as a warning and only enables Restore once the name is typed", async () => {
    planResult = basePlan();
    mount();
    fireEvent.click(screen.getByText("Choose a backup folder"));
    await waitFor(() => expect(screen.getByTestId("comparison")).toBeTruthy());
    await waitFor(() => expect(screen.getByTestId("warnings").textContent).toContain("different build"));
    expect(screen.queryByTestId("blocking")).toBeNull();

    const restore = screen.getByRole("button", { name: /Restore/ }) as HTMLButtonElement;
    expect(restore.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText("Type the hub's name to confirm"), {
      target: { value: "MyHub" },
    });
    expect(restore.disabled).toBe(false);
  });

  it("blocks when the backup holds a service this hub does not run", async () => {
    planResult = {
      ...basePlan(),
      services: [
        { ...basePlan().services[0] },
        { ...basePlan().services[1], deployed_image: null, deployed_image_id: null, verdict: "missing-in-target" },
      ],
      blocking: ["the backup holds data for mikro, which this hub does not run"],
      warnings: [],
    };
    mount();
    fireEvent.click(screen.getByText("Choose a backup folder"));
    await waitFor(() => expect(screen.getByTestId("blocking").textContent).toContain("mikro"));
    expect(screen.getByText("not deployed here")).toBeTruthy();

    fireEvent.change(screen.getByLabelText("Type the hub's name to confirm"), {
      target: { value: "MyHub" },
    });
    expect((screen.getByRole("button", { name: /Restore/ }) as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("reduceRestore", () => {
  it("folds verdicts into the health row", () => {
    let steps = reduceRestore({}, { event: "step", step: "health", title: "x" });
    steps = reduceRestore(steps, { event: "checked", service: "rekuest", healthy: true, detail: "answered 200" });
    expect(steps.health).toEqual({ status: "running", last: "rekuest: answered 200" });
  });
});
