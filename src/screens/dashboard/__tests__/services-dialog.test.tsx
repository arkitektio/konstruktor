import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  CreateEvent,
  DeploymentRecord,
  HubStatus,
  ServiceId,
  ServiceMeta,
} from "../../../api";
import { ServicesDialog, serviceDiff } from "../ServicesDialog";

const meta = (
  id: ServiceId,
  name: string,
  extra: Partial<ServiceMeta> = {}
): ServiceMeta => ({
  id,
  name,
  description: `${name}, in a line`,
  purpose: `What ${name} is for.`,
  default: true,
  emitted: true,
  experimental: false,
  ...extra,
});

const CATALOG: ServiceMeta[] = [
  meta("rekuest", "Rekuest"),
  meta("mikro", "Mikro"),
  meta("kraph", "Kraph"),
  meta("lovekit", "Lovekit", { emitted: false }),
  meta("bank", "Bank", { default: false, experimental: true }),
  meta("kuvert", "Kuvert", { default: false, experimental: true }),
];

const planServiceChange = vi.fn(
  async (_path: string, add: ServiceId[], remove: ServiceId[]) => ({
    added: add,
    removed: remove,
    unchanged: [] as ServiceId[],
    services: [] as ServiceId[],
    notes: ["Alpaka is added without a model provider"],
  })
);
const changeServices = vi.fn(
  async (
    options: { path: string; add: ServiceId[]; remove: ServiceId[]; apply: boolean },
    onEvent: (event: CreateEvent) => void
  ) => {
    onEvent({
      event: "staged",
      user_code: "A7K3",
      verification_uri_complete: "https://coord.example.org/hubconfigure/A7K3",
      expires_in: 300,
    });
    onEvent({ event: "granted", mesh_key: false });
    onEvent({ event: "starting" });
    onEvent({ event: "log", line: "Container myhub-bank-1  Started" });
    return {
      plan: {
        added: options.add,
        removed: options.remove,
        unchanged: [],
        services: ["rekuest", "mikro", "bank"] as ServiceId[],
        notes: [],
      },
      applied: options.apply,
      mesh_requested: false,
      mesh_granted: false,
    };
  }
);

vi.mock("@tauri-apps/plugin-shell", () => ({ open: () => undefined }));
vi.mock("../../../api", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  serviceCatalog: async () => CATALOG,
  planServiceChange: (path: string, add: ServiceId[], remove: ServiceId[]) =>
    planServiceChange(path, add, remove),
  changeServices: (
    options: { path: string; add: ServiceId[]; remove: ServiceId[]; apply: boolean },
    onEvent: (event: CreateEvent) => void
  ) => changeServices(options, onEvent),
  cancelAuthorization: async () => undefined,
}));

const DEPLOYMENT = {
  id: "abc",
  name: "MyHub",
  path: "/home/someone/MyHub",
  kind: "hub",
} as DeploymentRecord;

/** Only what the dialog reads: the services running now. */
const STATUS = {
  services: [
    { id: "rekuest", name: "Rekuest" },
    { id: "mikro", name: "Mikro" },
    { id: "kraph", name: "Kraph" },
  ],
} as unknown as HubStatus;

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const onDone = vi.fn();

const mount = () =>
  render(
    <ServicesDialog
      open
      deployment={DEPLOYMENT}
      status={STATUS}
      onOpenChange={() => undefined}
      onDone={onDone}
    />
  );

/** The card around a service's name — the highlight is the whole statement of "in". */
const card = (name: string) =>
  screen.getByText(name).closest('[data-slot="card"]') as HTMLElement;

describe("serviceDiff", () => {
  it("says what is added and removed, in catalog order", () => {
    expect(
      serviceDiff(["rekuest", "mikro", "kraph"], ["kuvert", "rekuest", "bank"], CATALOG)
    ).toEqual({ add: ["bank", "kuvert"], remove: ["mikro", "kraph"] });
    expect(serviceDiff(["mikro"], ["mikro"], CATALOG)).toEqual({ add: [], remove: [] });
  });
});

describe("ServicesDialog", () => {
  it("pre-ticks what the hub runs, and offers nothing that cannot run", async () => {
    mount();
    await screen.findByText("Mikro");

    expect(card("Rekuest").className).toContain("bg-primary/5");
    expect(card("Kraph").className).toContain("bg-primary/5");
    expect(screen.queryByText("Lovekit")).toBeNull();
    // Experimental ones are tucked away, and nothing has changed yet.
    expect(screen.queryByText("Bank")).toBeNull();
    expect(screen.queryByTestId("service-diff")).toBeNull();
    expect(
      (screen.getByRole("button", { name: "Apply changes" }) as HTMLButtonElement).disabled
    ).toBe(true);
  });

  it("summarises the change, says the data is kept, and asks the core about it", async () => {
    mount();
    await screen.findByText("Mikro");

    fireEvent.click(screen.getByText("Kraph"));
    fireEvent.click(screen.getByText("Experimental (2)"));
    fireEvent.click(screen.getByText("Bank"));

    const diff = screen.getByTestId("service-diff");
    expect(diff.textContent).toContain("Adding Bank");
    expect(diff.textContent).toContain("Removing Kraph");
    expect(diff.textContent).toContain("data is kept");
    expect(card("Kraph").className).not.toContain("bg-primary/5");

    await waitFor(() =>
      expect(planServiceChange).toHaveBeenLastCalledWith(DEPLOYMENT.path, ["bank"], ["kraph"])
    );
    expect(await screen.findByText("Alpaka is added without a model provider")).toBeTruthy();

    // Ticking it back undoes the change entirely.
    fireEvent.click(screen.getByText("Kraph"));
    fireEvent.click(screen.getByText("Bank"));
    expect(screen.queryByTestId("service-diff")).toBeNull();
  });

  it("shows why the core refuses a change, and will not apply it", async () => {
    planServiceChange.mockRejectedValueOnce(
      "Rekuest runs the periodic work and receives the signals of Mikro — remove those too, or keep Rekuest"
    );
    mount();
    await screen.findByText("Mikro");

    fireEvent.click(screen.getByText("Rekuest"));

    expect(await screen.findByText(/or keep Rekuest/)).toBeTruthy();
    expect(
      (screen.getByRole("button", { name: "Apply changes" }) as HTMLButtonElement).disabled
    ).toBe(true);
  });

  it("applies through the device code, streams the restart and reports back", async () => {
    mount();
    await screen.findByText("Mikro");
    fireEvent.click(screen.getByText("Experimental (2)"));
    fireEvent.click(screen.getByText("Bank"));
    await waitFor(() => expect(planServiceChange).toHaveBeenCalled());
    await waitFor(() =>
      expect(
        (screen.getByRole("button", { name: "Apply changes" }) as HTMLButtonElement).disabled
      ).toBe(false)
    );

    fireEvent.click(screen.getByRole("button", { name: "Apply changes" }));

    await screen.findByText("Services changed");
    expect(changeServices).toHaveBeenCalledWith(
      { path: DEPLOYMENT.path, add: ["bank"], remove: [], apply: true },
      expect.any(Function)
    );
    expect(screen.getByText(/myhub-bank-1/)).toBeTruthy();
    expect(screen.getByText(/now runs Rekuest, Mikro, Bank/)).toBeTruthy();
    expect(onDone).toHaveBeenCalled();
  });
});
