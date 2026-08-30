import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { FormProvider, useForm } from "react-hook-form";

import { StorageStep } from "../steps/StorageStep";
import type { StorageMode } from "../../../api";

afterEach(cleanup);

const Harness = ({ storage }: { storage: StorageMode }) => {
  const form = useForm({ defaultValues: { storage } });
  return (
    <FormProvider {...form}>
      <StorageStep />
    </FormProvider>
  );
};

describe("StorageStep", () => {
  it("recommends the volumes and shows no warning for them", () => {
    render(<Harness storage="docker-volumes" />);
    expect(screen.getByText("recommended")).toBeTruthy();
    expect(screen.queryByTestId("storage-warning")).toBeNull();
  });

  it("warns the moment the folder is picked, and stops when it is not", () => {
    render(<Harness storage="docker-volumes" />);
    fireEvent.click(screen.getByText("Folders inside the deployment"));
    expect(screen.getByTestId("storage-warning").textContent).toContain("slow");
    fireEvent.click(screen.getByText("Docker volumes"));
    expect(screen.queryByTestId("storage-warning")).toBeNull();
  });
});
