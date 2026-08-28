import { afterEach, describe, expect, it, vi } from "vitest";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { z } from "zod";
import { useFieldArray, useFormContext } from "react-hook-form";
import { Wizard, WizardStep } from "../Wizard";
import { ErrorDisplay } from "../../../components/Error";

afterEach(() => cleanup());

const nextButton = () => screen.getByText("next") as HTMLButtonElement;
const prevButton = () => screen.getByText("prev") as HTMLButtonElement;

const NameStep = () => {
  const { register } = useFormContext();
  return (
    <div>
      <input aria-label="name" {...register("name")} />
      <ErrorDisplay name="name" />
    </div>
  );
};

const GroupsStep = () => {
  const { register, control } = useFormContext();
  const { fields, append, remove } = useFieldArray({ control, name: "groups" });
  return (
    <div>
      {fields.map((f, i) => (
        <div key={f.id}>
          <input aria-label={`group-${i}`} {...register(`groups.${i}.name`)} />
          <button type="button" onClick={() => remove(i)}>
            remove-{i}
          </button>
        </div>
      ))}
      <button type="button" onClick={() => append({ name: "" })}>
        add-group
      </button>
      <ErrorDisplay name="groups" />
      <ErrorDisplay name="groups.root" />
    </div>
  );
};

const DoneStep = () => <div>done-step</div>;

const OptionalStep = () => <div>optional-step</div>;

const steps: WizardStep[] = [
  {
    component: NameStep,
    validationSchema: z.looseObject({
      name: z
        .string()
        .min(3, "Name must be at least 3 characters")
        .regex(/^[a-z]+$/, "Deployment names can be lowercase only"),
    }),
  },
  {
    component: GroupsStep,
    validationSchema: z.looseObject({
      groups: z
        .array(z.looseObject({ name: z.string().min(1, "Group name is required") }), {
          error: "At least one group is required",
        })
        .min(1, "At least one group is required")
        .refine((groups: { name: string }[]) => {
          const names = groups.map((g) => g.name);
          return names.length === new Set(names).size;
        }, "Group names must be unique"),
    }),
  },
  { component: DoneStep },
];

const renderWizard = (
  initialValues: Record<string, unknown>,
  onSubmit = vi.fn()
) => {
  const utils = render(
    <Wizard initialValues={initialValues} steps={steps} onSubmit={onSubmit}>
      {({ renderComponent, handleNext, handlePrev, isNextDisabled, isPrevDisabled, currentStepIndex }) => (
        <div>
          <div data-testid="step">{currentStepIndex}</div>
          {renderComponent()}
          <button disabled={isNextDisabled} onClick={handleNext}>
            next
          </button>
          <button disabled={isPrevDisabled} onClick={handlePrev}>
            prev
          </button>
        </div>
      )}
    </Wizard>
  );
  return { ...utils, onSubmit };
};

describe("Wizard", () => {
  it("validates on mount so prefilled defaults enable Next immediately", async () => {
    renderWizard({ name: "mydeployment", groups: [] });
    await waitFor(() =>
      expect(nextButton().disabled).toBe(false)
    );
  });

  it("keeps Next disabled while the active step is invalid and shows the message", async () => {
    renderWizard({ name: "AB", groups: [] });
    await waitFor(() =>
      expect(
        screen.getByText("Name must be at least 3 characters")
      ).toBeTruthy()
    );
    expect(nextButton().disabled).toBe(true);

    fireEvent.change(screen.getByLabelText("name"), {
      target: { value: "valid" },
    });
    await waitFor(() => expect(nextButton().disabled).toBe(false));
  });

  it("advances, applies the next step's schema, and keeps earlier values", async () => {
    const { onSubmit } = renderWizard({ name: "mydeployment", groups: [] });
    await waitFor(() => expect(nextButton().disabled).toBe(false));

    await act(async () => {
      fireEvent.click(nextButton());
    });
    expect(screen.getByTestId("step").textContent).toBe("1");

    // step 2 schema requires at least one group
    await waitFor(() =>
      expect(screen.getByText("At least one group is required")).toBeTruthy()
    );
    expect(nextButton().disabled).toBe(true);

    await act(async () => {
      fireEvent.click(screen.getByText("add-group"));
    });
    await act(async () => {
      fireEvent.change(screen.getByLabelText("group-0"), {
        target: { value: "myteam" },
      });
    });
    await waitFor(() => expect(nextButton().disabled).toBe(false));

    // duplicate group names are rejected by the array level refine
    await act(async () => {
      fireEvent.click(screen.getByText("add-group"));
    });
    await act(async () => {
      fireEvent.change(screen.getByLabelText("group-1"), {
        target: { value: "myteam" },
      });
    });
    await waitFor(() =>
      expect(screen.getByText("Group names must be unique")).toBeTruthy()
    );
    expect(nextButton().disabled).toBe(true);

    await act(async () => {
      fireEvent.click(screen.getByText("remove-1"));
    });
    await waitFor(() => expect(nextButton().disabled).toBe(false));

    await act(async () => {
      fireEvent.click(nextButton());
    });
    expect(screen.getByText("done-step")).toBeTruthy();

    // last step submits with every value collected along the way
    await act(async () => {
      fireEvent.click(nextButton());
    });
    await waitFor(() => expect(onSubmit).toHaveBeenCalled());
    expect(onSubmit.mock.calls[0][0]).toMatchObject({
      name: "mydeployment",
      groups: [{ name: "myteam" }],
    });
  });

  it("skips steps that do not apply to the answers so far", async () => {
    const onSubmit = vi.fn();
    const conditional: WizardStep[] = [
      steps[0],
      {
        component: OptionalStep,
        enabled: (values) => Boolean(values.wantsExtra),
        validationSchema: z.looseObject({
          // would fail if it were ever applied to these values
          neverSet: z.string(),
        }),
      },
      { component: DoneStep },
    ];

    render(
      <Wizard
        initialValues={{ name: "mydeployment", wantsExtra: false }}
        steps={conditional}
        onSubmit={onSubmit}
      >
        {({ renderComponent, handleNext, handlePrev, isPrevDisabled, isLastStep }) => (
          <div>
            {renderComponent()}
            <div data-testid="last">{String(isLastStep)}</div>
            <button disabled={isPrevDisabled} onClick={handlePrev}>
              prev
            </button>
            <button onClick={handleNext}>next</button>
          </div>
        )}
      </Wizard>
    );

    await waitFor(() => expect(nextButton().disabled).toBe(false));

    await act(async () => {
      fireEvent.click(nextButton());
    });
    // the optional step is skipped in both directions, and its schema never runs
    expect(screen.getByText("done-step")).toBeTruthy();
    expect(screen.getByTestId("last").textContent).toBe("true");

    await act(async () => {
      fireEvent.click(prevButton());
    });
    expect(screen.getByLabelText("name")).toBeTruthy();
  });

  it("visits a step once its condition holds", async () => {
    const conditional: WizardStep[] = [
      steps[0],
      {
        component: OptionalStep,
        enabled: (values) => Boolean(values.wantsExtra),
      },
      { component: DoneStep },
    ];

    render(
      <Wizard
        initialValues={{ name: "mydeployment", wantsExtra: true }}
        steps={conditional}
        onSubmit={vi.fn()}
      >
        {({ renderComponent, handleNext, isLastStep }) => (
          <div>
            {renderComponent()}
            <div data-testid="last">{String(isLastStep)}</div>
            <button onClick={handleNext}>next</button>
          </div>
        )}
      </Wizard>
    );

    await waitFor(() => expect(nextButton().disabled).toBe(false));
    expect(screen.getByTestId("last").textContent).toBe("false");

    await act(async () => {
      fireEvent.click(nextButton());
    });
    expect(screen.getByText("optional-step")).toBeTruthy();
  });

  it("goes back to previous steps", async () => {
    renderWizard({ name: "mydeployment", groups: [{ name: "myteam" }] });
    await waitFor(() => expect(nextButton().disabled).toBe(false));
    expect(prevButton().disabled).toBe(true);

    await act(async () => {
      fireEvent.click(nextButton());
    });
    expect(screen.getByTestId("step").textContent).toBe("1");
    expect(prevButton().disabled).toBe(false);

    await act(async () => {
      fireEvent.click(prevButton());
    });
    expect(screen.getByTestId("step").textContent).toBe("0");
    expect((screen.getByLabelText("name") as HTMLInputElement).value).toBe(
      "mydeployment"
    );
  });
});
