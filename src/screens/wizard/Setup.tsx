import { FormikHelpers, FormikValues } from "formik";
import { FormikWizard, RenderProps } from "formik-wizard-form";
import React from "react";
import { Link, useNavigate } from "react-router-dom";
import * as Yup from "yup";
import { useCommunication } from "../../communication/communication-context";
import { Button } from "../../components/ui/button";
import { useBindings } from "../../interface/context";
import { Page } from "../../layout/Page";
import { useStorage } from "../../storage/storage-context";
import { ChannelForm } from "./forms/ChannelForm";
import { CheckDocker } from "./forms/CheckDocker";
import { Greeting } from "./forms/Greeting";

export type ChooseSetup = {
  name: string;
  channel: string;
};

export const debugUser = {
  name: "debug",
  username: "debug",
  password: "debug",
  email: "debug@debug.com",
  groups: ["myteam"],
};
































export const Setup: React.FC<{}> = (props) => {
  const { call } = useCommunication();
  const { installApp, apps } = useStorage();
  const { bindings } = useBindings();
  const navigate = useNavigate();

  const basicSetup: Partial<ChooseSetup> = {
    name: "mydeployment",
  };

  const handleSubmit = async (
    values: FormikValues,
    formikHelpers: FormikHelpers<FormikValues>
  ) => {
    if (values) {
      formikHelpers.setSubmitting(true);
      navigate("/channelsetup/" + values.name + "/" + values.channel);
    }
  };

  return (
    <FormikWizard
      initialValues={basicSetup}
      onSubmit={handleSubmit}
      validateOnNext
      validateOnMount
      validateOnChange
      activeStepIndex={0}
      steps={[
        {
          component: Greeting,
          validationSchema: Yup.object().shape({
            name: Yup.string()
              .required("Name is required")
              .min(3, "Name must be at least 3 characters")
              .max(20, "Name must be at most 20 characters")
              .test(
                "unique",
                "Another app with this already exists",
                (value) => {
                  return !apps.find((app) => app.name === value);
                }
              )
              .matches(/^[a-z]+$/, "Deployment names can be lowercase only"),
          }),
        },
        {
          component: CheckDocker,
        },
        {
          component: ChannelForm,
        },
      ]}
    >
      {({
        currentStepIndex,
        renderComponent,
        handlePrev,
        handleNext,
        isSubmitting,
        isNextDisabled,
        isPrevDisabled,
      }: RenderProps) => {
        return (
          <Page
            buttons={
              <>
                <Button disabled={isNextDisabled} onClick={handleNext}>
                  {isSubmitting ? "Building ..." : "Next"}
                </Button>
                {currentStepIndex == 0 ? (
                  <Button>
                    <Link to="/" className="w-full">
                      Go back
                    </Link>
                  </Button>
                ) : (
                  <Button disabled={isPrevDisabled} onClick={handlePrev}>
                    Prev
                  </Button>
                )}
              </>
            }
          >
            {renderComponent()}
          </Page>
        );
      }}
    </FormikWizard>
  );
};
