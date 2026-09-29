import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { FormProvider, useForm, useWatch } from "react-hook-form";

import { ServicesStep } from "../steps/ServicesStep";
import type { ServiceId, ServiceMeta } from "../../../api";

const meta = (
  id: ServiceId,
  name: string,
  description: string,
  extra: Partial<ServiceMeta> = {}
): ServiceMeta => ({
  id,
  name,
  description,
  purpose: `What ${name} is for.`,
  default: true,
  emitted: true,
  experimental: false,
  ...extra,
});

const CATALOG: ServiceMeta[] = [
  meta("rekuest", "Rekuest", "Task orchestration and workflow execution"),
  meta("mikro", "Mikro", "Microscopy data management and analysis"),
  meta("bank", "Bank", "Bank accounts, transactions and budgets", {
    default: false,
    experimental: true,
  }),
  meta("kuvert", "Kuvert", "Your mailboxes, synced and searchable", {
    default: false,
    experimental: true,
  }),
];

vi.mock("../../../api", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  serviceCatalog: async () => CATALOG,
}));

afterEach(cleanup);

/** Shows the form's `services` value, which is what the wizard hands the core. */
const Selected = () => {
  const services = (useWatch({ name: "services" }) ?? []) as ServiceId[];
  return <output data-testid="selected">{services.join(",")}</output>;
};

const Harness = () => {
  const form = useForm({
    defaultValues: {
      services: [] as ServiceId[],
      rekuestServer: "local",
      serviceOptions: {},
    },
  });
  return (
    <FormProvider {...form}>
      <ServicesStep />
      <Selected />
    </FormProvider>
  );
};

const selected = () => screen.getByTestId("selected").textContent;

describe("ServicesStep", () => {
  it("keeps the experimental services out of sight until asked for", async () => {
    render(<Harness />);
    await screen.findByText("Mikro");

    expect(screen.queryByText("Bank")).toBeNull();
    expect(screen.queryByText("Kuvert")).toBeNull();

    fireEvent.click(screen.getByText("Experimental (2)"));

    expect(screen.getByText("Bank")).toBeTruthy();
    expect(screen.getByText("Kuvert")).toBeTruthy();
    expect(
      screen.getByText("Bank accounts, transactions and budgets")
    ).toBeTruthy();
    expect(screen.getByText("Your mailboxes, synced and searchable")).toBeTruthy();
  });

  it("does not pre-tick them, and ticking one adds it to the form", async () => {
    render(<Harness />);
    await screen.findByText("Mikro");

    // The defaults are ticked, the experimental services are not.
    expect(selected()).toBe("rekuest,mikro");

    fireEvent.click(screen.getByText("Experimental (2)"));
    fireEvent.click(screen.getByText("Kuvert"));

    // In catalog order, wherever in the list it was clicked.
    expect(selected()).toBe("rekuest,mikro,kuvert");
  });

  it("moves the panel back to a listed service when the section is closed", async () => {
    render(<Harness />);
    await screen.findByText("Mikro");

    fireEvent.click(screen.getByText("Experimental (2)"));
    fireEvent.click(screen.getByText("Bank"));
    expect(screen.getAllByText("Bank").length).toBe(2); // the row, and the panel

    fireEvent.click(screen.getByText("Experimental (2)"));
    expect(screen.queryByText("Bank")).toBeNull();
    expect(screen.getByText("What Rekuest is for.")).toBeTruthy();
  });
});
