import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { DeploymentRecord } from "../../../api";
import { TooltipProvider } from "../../../components/ui/tooltip";
import { RegistryContext } from "../../../registry/registry-context";
import { DeploymentMenu } from "../DeploymentMenu";

/**
 * The menu is where every non-obvious action on a deployment now lives, including the
 * three that destroy something. What is worth pinning down is the confirmation flow: a
 * confirmation opened *from* a menu item has to survive the menu closing, which a popover
 * anchored to the item would not. That failure is silent — the item looks like it simply
 * does nothing — so it is asserted rather than eyeballed.
 */

const forget = vi.fn(async () => undefined);
const refresh = vi.fn(async () => undefined);
const openShell = vi.fn((_path: string) => undefined);
const composeCommand = vi.fn(async (_path: string, _action: string) => undefined);
const deleteDeployment = vi.fn(async (_id: string) => ({
  path: "/home/someone/MyHub",
  stack_removed: true,
  folder_removed: true,
  forgotten: true,
}));
const planDeletion = vi.fn(async (_id: string) => ({
  path: "/home/someone/MyHub",
  name: "MyHub",
  checkouts: ["rekuest", "mikro"],
  was_authorized: true,
  data_dirs: ["/home/someone/MyHub/db_data", "/home/someone/MyHub/minio_data"],
  skipped: [] as { mount: string; explanation: string }[],
  on_a_mesh: false,
  storage: "deployment-folder" as const,
}));
const purgeDeploymentData = vi.fn(async (_id: string) => ({
  path: "/home/someone/MyHub",
  stack_removed: true,
  removed: ["/home/someone/MyHub/db_data", "/home/someone/MyHub/minio_data"],
  skipped: [],
}));

vi.mock("@tauri-apps/plugin-shell", () => ({
  open: (path: string) => openShell(path),
}));

vi.mock("../../../api", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  composeCommand: (path: string, action: string) => composeCommand(path, action),
  // The menu's actions stream now; the test only cares that the command was issued.
  composeCommandStreamed: (path: string, action: string) => composeCommand(path, action),
  deleteDeployment: (id: string) => deleteDeployment(id),
  planDeletion: (id: string) => planDeletion(id),
  purgeDeploymentData: (id: string) => purgeDeploymentData(id),
}));

const DEPLOYMENT: DeploymentRecord = {
  id: "abc",
  name: "MyHub",
  path: "/home/someone/MyHub",
  kind: "hub",
  project: "myhub",
  createdAt: "2026-01-01T00:00:00Z",
};

beforeAll(() => {
  // Radix's menus drive themselves off pointer APIs jsdom does not implement.
  Element.prototype.hasPointerCapture ??= () => false;
  Element.prototype.setPointerCapture ??= () => undefined;
  Element.prototype.releasePointerCapture ??= () => undefined;
  Element.prototype.scrollIntoView ??= () => undefined;
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const mount = () =>
  render(
    <MemoryRouter>
      {/* Both are mounted app-wide in `App.tsx`; the menu leans on each. */}
      <TooltipProvider>
      <RegistryContext.Provider
        value={{
          deployments: [DEPLOYMENT],
          loading: false,
          pickFolder: async () => undefined,
          suggestFolder: async () => undefined,
          inspectFolder: async () => ({ ok: true, message: "" }),
          forget,
          byId: () => DEPLOYMENT,
          refresh,
        }}
      >
        <DeploymentMenu
          deployment={DEPLOYMENT}
          onRefresh={() => undefined}
          onReload={() => undefined}
        />
      </RegistryContext.Provider>
      </TooltipProvider>
    </MemoryRouter>
  );

/**
 * Opened from the keyboard: Radix's mouse path goes through `pointerdown` with a real
 * `PointerEvent`, which jsdom does not have. The keyboard path opens exactly the same
 * menu, and it is the one an accessibility check would take anyway.
 */
const openMenu = async () => {
  fireEvent.keyDown(screen.getByRole("button", { name: "Deployment actions" }), {
    key: "Enter",
  });
  await screen.findByRole("menu");
};

/** Menu items commit on `pointerup`; a bare click is not enough in jsdom. */
const choose = (name: string) => {
  const item = screen.getByRole("menuitem", { name });
  fireEvent.pointerUp(item);
  fireEvent.click(item);
};

describe("the deployment menu", () => {
  it("offers every action the page no longer carries itself", async () => {
    mount();
    await openMenu();

    // "Open in browser" is gone on purpose: Orkestrator is how a hub gets opened.
    for (const label of [
      "Open folder",
      "Logs",
      "Reload",
      "Authorize",
      "Remove containers",
      "Delete all data",
      "Forget deployment",
      "Delete hub",
    ]) {
      expect(screen.getByRole("menuitem", { name: label })).toBeTruthy();
    }
  });

  it("acts on a plain item without confirming", async () => {
    mount();
    await openMenu();

    choose("Open folder");
    expect(openShell).toHaveBeenCalledWith(DEPLOYMENT.path);
  });

  it("keeps a destructive item's confirmation alive after the menu closes", async () => {
    mount();
    await openMenu();

    choose("Delete all data");

    const dialog = await screen.findByRole("dialog");
    expect(dialog.textContent).toContain("Delete all data?");
    // The copy has to name what is lost — this is the one action nothing undoes.
    expect(dialog.textContent).toContain("for good");
    // …and what survives, which is the half the old copy got wrong.
    expect(dialog.textContent).toContain("folder stays");
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("runs nothing when the confirmation is cancelled, and leaves the page usable", async () => {
    mount();
    await openMenu();
    choose("Forget deployment");
    await screen.findByRole("dialog");

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(forget).not.toHaveBeenCalled();
    // A tripwire, not a proof: the reason the menu is `modal={false}` is that Radix
    // hands the body's pointer events back on a timer, which a dialog opening in the same
    // tick can race. jsdom does not reproduce that race — this assertion passes either
    // way — so it is here to catch a regression cheaply, not to demonstrate the fix.
    expect(document.body.style.pointerEvents).not.toBe("none");
  });

  it("goes through with a confirmed forget", async () => {
    mount();
    await openMenu();
    choose("Forget deployment");
    await screen.findByRole("dialog");

    fireEvent.click(screen.getByRole("button", { name: "Forget" }));

    await waitFor(() => expect(forget).toHaveBeenCalledWith(DEPLOYMENT.id));
    // Not `refresh` as well: the registry's own `forget` reloads the list, and doing it
    // again here is what used to flash "Unknown deployment" before the navigation landed.
    expect(refresh).not.toHaveBeenCalled();
  });

  it("confirms a destructive compose action before running it", async () => {
    mount();
    await openMenu();
    choose("Remove containers");
    await screen.findByRole("dialog");

    expect(composeCommand).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));

    await waitFor(() =>
      expect(composeCommand).toHaveBeenCalledWith(DEPLOYMENT.path, "down")
    );
  });
});

/**
 * Deleting a hub is the one action with nothing behind it — no volume left, no folder, no
 * entry. The gate in front of it is the deployment's own name, typed out, and these are
 * the three things that gate has to do.
 */
describe("deleting a hub outright", () => {
  const openDelete = async () => {
    mount();
    await openMenu();
    choose("Delete hub");
    return screen.findByRole("dialog");
  };

  it("will not delete until the name is typed exactly", async () => {
    await openDelete();
    const confirm = screen.getByRole("button", { name: "Delete this hub" });

    expect((confirm as HTMLButtonElement).disabled).toBe(true);

    // Close, but not it. Case included — this is the whole point of the gate.
    for (const wrong of ["My", "myhub", "MyHub "]) {
      fireEvent.change(screen.getByRole("textbox"), { target: { value: wrong } });
      expect((confirm as HTMLButtonElement).disabled).toBe(true);
    }

    fireEvent.click(confirm);
    expect(deleteDeployment).not.toHaveBeenCalled();
  });

  it("deletes once the name matches", async () => {
    await openDelete();
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "MyHub" } });

    const confirm = screen.getByRole("button", { name: "Delete this hub" });
    expect((confirm as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(confirm);

    await waitFor(() => expect(deleteDeployment).toHaveBeenCalledWith(DEPLOYMENT.id));
    await waitFor(() => expect(refresh).toHaveBeenCalled());
  });

  it("says what else goes with it, so nothing is a surprise", async () => {
    const dialog = await openDelete();

    expect(dialog.textContent).toContain(DEPLOYMENT.path);
    expect(dialog.textContent).toContain("database and object storage");
    // The plan is fetched once the dialog is open, and adds what the registry cannot know.
    await waitFor(() => expect(dialog.textContent).toContain("rekuest, mikro"));
    expect(dialog.textContent).toContain("coordination server");
    expect(dialog.textContent).toContain("images stay");
  });

  it("keeps the dialog open and shows why when the delete fails", async () => {
    deleteDeployment.mockRejectedValueOnce("Docker could not take the stack down");
    await openDelete();
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "MyHub" } });
    fireEvent.click(screen.getByRole("button", { name: "Delete this hub" }));

    await waitFor(() =>
      expect(screen.getByRole("dialog").textContent).toContain(
        "Docker could not take the stack down"
      )
    );
    expect(screen.queryByRole("dialog")).not.toBeNull();
  });
});

/**
 * The bug this whole change exists for: "Delete all data" ran
 * `docker compose down --volumes`, which removed nothing, because the database and object
 * storage are bind mounts inside the deployment folder and the stack declares no named
 * volumes. These pin the new behaviour so it cannot quietly regress to a no-op.
 */
describe("deleting a hub's data", () => {
  const openPurge = async () => {
    mount();
    await openMenu();
    choose("Delete all data");
    return screen.findByRole("dialog");
  };

  const type = (value: string) =>
    fireEvent.change(screen.getByRole("textbox"), { target: { value } });

  it("goes through its own command and never through compose", async () => {
    await openPurge();
    type("MyHub");
    fireEvent.click(screen.getByRole("button", { name: "Delete the data" }));

    await waitFor(() =>
      expect(purgeDeploymentData).toHaveBeenCalledWith(DEPLOYMENT.id)
    );
    // The regression guard: `down --volumes` is not a way to delete data and must not
    // come back as one.
    expect(composeCommand).not.toHaveBeenCalled();
  });

  it("will not run until the hub's name is typed", async () => {
    await openPurge();
    const confirm = screen.getByRole("button", { name: "Delete the data" });
    expect((confirm as HTMLButtonElement).disabled).toBe(true);

    type("myhub");
    expect((confirm as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(confirm);
    expect(purgeDeploymentData).not.toHaveBeenCalled();
  });

  it("names the directories it will delete rather than guessing them", async () => {
    const dialog = await openPurge();

    // `db_data` and `minio_data` are defaults, not constants — the core resolves them and
    // the dialog shows what it resolved, so a hub that keeps data elsewhere reads true.
    await waitFor(() =>
      expect(dialog.textContent).toContain("/home/someone/MyHub/db_data")
    );
    expect(dialog.textContent).toContain("/home/someone/MyHub/minio_data");
  });

  it("says what survives, which is the half the old copy got wrong", async () => {
    const dialog = await openPurge();
    expect(dialog.textContent).toContain("folder stays");
    expect(dialog.textContent).toContain("docker-compose.yaml");
  });

  it("keeps the dialog open and shows why when the purge fails", async () => {
    // The real failure from the machine that produced this bug.
    purgeDeploymentData.mockRejectedValueOnce(
      "Permission denied (os error 13)"
    );
    await openPurge();
    type("MyHub");
    fireEvent.click(screen.getByRole("button", { name: "Delete the data" }));

    await waitFor(() =>
      expect(screen.getByRole("dialog").textContent).toContain(
        "Permission denied (os error 13)"
      )
    );
  });

  it("warns that a mesh hub loses its place on the tailnet", async () => {
    planDeletion.mockResolvedValueOnce({
      path: "/home/someone/MyHub",
      name: "MyHub",
      checkouts: [],
      was_authorized: true,
      data_dirs: ["/home/someone/MyHub/db_data"],
      skipped: [],
      on_a_mesh: true,
      storage: "deployment-folder" as const,
    });
    const dialog = await openPurge();
    await waitFor(() => expect(dialog.textContent).toContain("tailnet"));
  });

  it("names data it refuses to touch instead of silently leaving it", async () => {
    planDeletion.mockResolvedValueOnce({
      path: "/home/someone/MyHub",
      name: "MyHub",
      checkouts: [],
      was_authorized: false,
      data_dirs: ["/home/someone/MyHub/db_data"],
      skipped: [
        {
          mount: "/data",
          explanation: "it is an absolute path, outside the deployment folder",
        },
      ],
      on_a_mesh: false,
      storage: "deployment-folder" as const,
    });
    const dialog = await openPurge();
    await waitFor(() => expect(dialog.textContent).toContain("/data"));
    expect(dialog.textContent).toContain("will not touch");
  });
});
