import { zodResolver } from "@hookform/resolvers/zod";
import React, { useCallback, useEffect, useRef, useState } from "react";
import {
  FieldValues,
  FormProvider,
  Resolver,
  SubmitHandler,
  useForm,
} from "react-hook-form";
import type { ZodType } from "zod";

/** What the progress rail and the step frame show for a step. */
export type WizardStepMeta = {
  /** Short label for the rail, e.g. "Docker". */
  label: string;
  /** The step's own heading. */
  title: string;
  /** One line under the heading. */
  subtitle?: string;
  icon?: React.ComponentType<{ className?: string }>;
};

export type WizardStep = {
  component: React.ComponentType<any>;
  meta?: WizardStepMeta;
  validationSchema?: ZodType<any, any>;
  /**
   * Whether this step applies to the answers so far. A step that does not apply is
   * skipped in both directions and never validated — that is how a question like
   * "set up organizations now?" removes the questions it makes irrelevant.
   */
  enabled?: (values: FieldValues) => boolean;
};

/** One entry of the progress rail: an applicable step and where the user is in it. */
export type WizardRailStep = {
  index: number;
  meta?: WizardStepMeta;
  status: "done" | "current" | "upcoming";
};

export type WizardRenderProps = {
  currentStepIndex: number;
  /** The metadata of the step being shown, if it declared any. */
  currentStep?: WizardStepMeta;
  /** Only the steps that apply to the answers so far, in order. */
  rail: WizardRailStep[];
  /** The current step's position among the applicable ones, 1-based. */
  position: number;
  /** How many steps apply to the answers so far. */
  total: number;
  renderComponent: () => React.ReactNode;
  handlePrev: () => void;
  handleNext: () => void;
  /** Jump straight to an earlier step. Forward jumps are refused. */
  goBackTo: (index: number) => void;
  isSubmitting: boolean;
  isNextDisabled: boolean;
  isPrevDisabled: boolean;
  /** No applicable step follows: "Next" submits. */
  isLastStep: boolean;
};

export type WizardProps<T extends FieldValues> = {
  initialValues: T;
  steps: WizardStep[];
  onSubmit: SubmitHandler<T>;
  children: (props: WizardRenderProps) => React.ReactNode;
};

/**
 * A small multi step form harness on top of react-hook-form.
 *
 * Each step can carry its own zod schema. Because react-hook-form binds a
 * single resolver for the lifetime of the form, the resolver closes over the
 * currently active step and delegates to that step's schema. Steps without a
 * schema always validate.
 *
 * The resolver runs in `raw` mode so that values of other steps are never
 * stripped by the active step's schema.
 */
export const Wizard = <T extends FieldValues>({
  initialValues,
  steps,
  onSubmit,
  children,
}: WizardProps<T>) => {
  const [currentStepIndex, setCurrentStepIndex] = useState(0);
  const stepIndexRef = useRef(0);
  const stepsRef = useRef(steps);
  stepsRef.current = steps;

  const resolver: Resolver<T> = useCallback(
    async (values, context, options) => {
      const step = stepsRef.current[stepIndexRef.current];
      const schema = step?.validationSchema;
      if (!schema || (step?.enabled && !step.enabled(values))) {
        return { values, errors: {} };
      }
      return zodResolver(schema, undefined, { raw: true })(
        values,
        context,
        options as any
      ) as any;
    },
    []
  );

  const form = useForm<T>({
    defaultValues: initialValues as any,
    mode: "onChange",
    reValidateMode: "onChange",
    resolver,
  });

  const { trigger, handleSubmit, formState, watch, getValues } = form;

  const isEnabled = useCallback(
    (index: number) => {
      const step = stepsRef.current[index];
      if (!step) return false;
      return step.enabled ? step.enabled(getValues()) : true;
    },
    [getValues]
  );

  /** The next applicable step in `direction`, or undefined when there is none. */
  const seek = useCallback(
    (from: number, direction: 1 | -1) => {
      for (
        let index = from + direction;
        index >= 0 && index < stepsRef.current.length;
        index += direction
      ) {
        if (isEnabled(index)) return index;
      }
      return undefined;
    },
    [isEnabled]
  );

  // formik-wizard-form validated on mount, so a step that is already valid
  // (e.g. through prefilled defaults) enables "Next" on the first render.
  useEffect(() => {
    trigger();
  }, [currentStepIndex, trigger]);

  // react-hook-form only applies the error of the field that changed, so errors
  // that a step schema reports on another path (e.g. the root of a field array)
  // would stay invisible until the next submit. Revalidating the whole step on
  // every change keeps the formik behaviour of showing them right away.
  useEffect(() => {
    const subscription = watch(() => {
      trigger();
    });
    return () => subscription.unsubscribe();
  }, [watch, trigger]);

  const goToStep = useCallback((index: number) => {
    stepIndexRef.current = index;
    setCurrentStepIndex(index);
  }, []);

  const handleNext = useCallback(async () => {
    const valid = await trigger();
    if (!valid) return;

    const next = seek(stepIndexRef.current, 1);
    if (next === undefined) {
      await handleSubmit(onSubmit)();
      return;
    }

    goToStep(next);
  }, [trigger, handleSubmit, onSubmit, goToStep, seek]);

  const handlePrev = useCallback(() => {
    const previous = seek(stepIndexRef.current, -1);
    if (previous === undefined) return;
    goToStep(previous);
  }, [goToStep, seek]);

  /**
   * Backwards only. Every step forward has to pass its own schema, and a step the user
   * has not reached yet may not even apply — `enabled` is answered from values that
   * have not been given.
   */
  const goBackTo = useCallback(
    (index: number) => {
      if (index >= stepIndexRef.current || !isEnabled(index)) return;
      goToStep(index);
    },
    [goToStep, isEnabled]
  );

  const renderComponent = useCallback(() => {
    const step = steps[currentStepIndex];
    if (!step) return null;
    const StepComponent = step.component;
    return <StepComponent />;
  }, [steps, currentStepIndex]);

  const rail: WizardRailStep[] = steps
    .map((step, index) => ({ step, index }))
    .filter(({ index }) => isEnabled(index))
    .map(({ step, index }) => ({
      index,
      meta: step.meta,
      status:
        index === currentStepIndex
          ? ("current" as const)
          : index < currentStepIndex
            ? ("done" as const)
            : ("upcoming" as const),
    }));

  return (
    <FormProvider {...form}>
      <form onSubmit={(e) => e.preventDefault()}>
        {children({
          currentStepIndex,
          currentStep: steps[currentStepIndex]?.meta,
          rail,
          position: rail.findIndex((s) => s.index === currentStepIndex) + 1,
          total: rail.length,
          renderComponent,
          handlePrev,
          handleNext,
          goBackTo,
          isSubmitting: formState.isSubmitting,
          isNextDisabled: !formState.isValid || formState.isSubmitting,
          isPrevDisabled:
            seek(currentStepIndex, -1) === undefined || formState.isSubmitting,
          isLastStep: seek(currentStepIndex, 1) === undefined,
        })}
      </form>
    </FormProvider>
  );
};
