import {
  FieldProps,
  FormikErrors,
  FormikHandlers,
  FormikHelpers,
  FormikValues,
} from "formik";
import { FormikWizard, RenderProps } from "formik-wizard-form";
import React from "react";
import { useCommunication } from "../../communication/communication-context";
import * as Yup from "yup";
import { Link, useNavigate } from "react-router-dom";
import { stringify } from "yaml";
import { useStorage } from "../../storage/storage-context";
import { AdminUserForm } from "./forms/AdminUserForm";
import { Greeting } from "./forms/Greeting";
import { CheckDocker } from "./forms/CheckDocker";
import { ServiceSelection } from "./forms/ServiceSelection";
import { AppStorage } from "./forms/AppStorage";
import { AppSelection } from "./forms/AppSelection";
import { AttentionSuperuser } from "./forms/AttentionSuperuser";
import { Done } from "./forms/Done";
import { App } from "./fields/AppSelectionField";
import { Service } from "./fields/ServiceSelectionField";
import { AdverstisedHostsForm } from "./forms/AdverstisedHostsForm";
import { Group, GroupsForm } from "./forms/GroupsForm";
import { User, UsersForm } from "./forms/UsersForm";
import { ScaleForm } from "./forms/ScaleForm";
import { Scale, scaleOptions } from "./fields/ScaleField";

export type SetupValues = {
  name: string;
  admin_username: string;
  admin_password: string;
  admin_email: string;
  attention: boolean;
  apps: App[];
  bindings: string[];
  services: Service[];
  app_path: string;
  groups: Group[];
  users: User[];
  scale: Scale;
};

export const basicSetup: SetupValues = {
  name: "default",
  admin_username: "",
  admin_password: "",
  admin_email: "",
  attention: false,
  apps: [],
  bindings: [],
  services: [],
  app_path: "",
  groups: [{ name: "myteam", description: "My Perfect Team" }],
  users: [],
  scale: scaleOptions[0],
};

export const Setup: React.FC<{}> = (props) => {
  const { call } = useCommunication();
  const { installApp, apps } = useStorage();
  const navigate = useNavigate();

  const handleSubmit = async (
    values: FormikValues,
    formikHelpers: FormikHelpers<FormikValues>
  ) => {
    if (values.app_path) {
      let sendapp = {
        name: values.name,
        dirpath: values.app_path,
        yaml: stringify(values),
      };

      formikHelpers.setSubmitting(true);

      let res = await call<any, { ok: string; error: string }>(
        "directory_init_cmd",
        sendapp
      );

      formikHelpers.setSubmitting(false);

      if (res.ok) {
        installApp(values as SetupValues);
        navigate("/");
      }
      if (res.error) {
        alert(res.error);
      }
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
              .lowercase("Lowercase only")
              .matches(/^[a-z]+$/, "Lowercase only"),
          }),
        },
        {
          component: CheckDocker,
        },

        {
          component: ScaleForm,
        },
        {
          component: AppStorage,
          validationSchema: Yup.object().shape({
            app_path: Yup.string().required("App path is required"),
          }),
        },
        {
          component: ServiceSelection,
          validationSchema: Yup.object().shape({
            services: Yup.array().required("Desired Modules Required"),
          }),
        },
        {
          component: AppSelection,
          validationSchema: Yup.object().shape({
            apps: Yup.array().required("Desired Modules Required"),
          }),
        },
        {
          component: AttentionSuperuser,
          validationSchema: Yup.object().shape({
            attention: Yup.bool()
              .isTrue()
              .required("You need to understand this"),
          }),
        },
        {
          component: AdminUserForm,
          validationSchema: Yup.object().shape({
            admin_email: Yup.string()
              .email()
              .required("User Email is required"),
            admin_username: Yup.string().required("Username is required"),
            admin_password: Yup.string().required("Password is required"),
          }),
        },
        {
          component: GroupsForm,
          validationSchema: Yup.object().shape({
            groups: Yup.array(
              Yup.object().shape({
                name: Yup.string()
                  .lowercase()
                  .matches(/^[a-z]+$/, "No special characters allowd")
                  .required("Username is required"),
                description: Yup.string().required("Password iss required"),
              })
            )
              .required("Desired Modules Required")
              .test(
                "unique",
                "Group names must be unique",
                (groups: Group[]) => {
                  let names = groups.map((g) => g.name);
                  return names.length === new Set(names).size;
                }
              ),
          }),
        },
        {
          component: UsersForm,
          validationSchema: Yup.object().shape({
            users: Yup.array(
              Yup.object().shape({
                email: Yup.string().email().required("User Email is required"),
                username: Yup.string().required("Username is required"),
                password: Yup.string().required("Password is required"),
                groups: Yup.array().required("Groups are required"),
              })
            )
              .required("Desired Modules Required")
              .test("unique", "Usernames must be unique", (users: User[]) => {
                let names = users.map((g) => g.username);
                return names.length === new Set(names).size;
              }),
          }),
        },

        {
          component: AdverstisedHostsForm,
          validationSchema: Yup.object().shape({
            bindings: Yup.array().min(1).required("Bindings required"),
          }),
        },

        {
          component: Done,
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
          <div className="flex-grow flex flex-col p-5">
            <div className="flex flex-grow">
              <div className="flex-none"></div>
              <div className="flex-grow flex text-justify">
                {renderComponent()}
              </div>
              <div className="flex-initial"></div>
            </div>
            <div className="bg-red flex-initial text-center pb-3 gap-2 grid grid-cols-2">
              {currentStepIndex == 0 ? (
                <Link
                  to="/"
                  className="border rounded shadow-xl shadow-cyan-400/20 border-cyan-500 p-1 disabled:invisible"
                >
                  Go back
                </Link>
              ) : (
                <button
                  disabled={isPrevDisabled}
                  className="border rounded shadow-xl shadow-cyan-400/20 border-cyan-500 p-1 disabled:invisible"
                  onClick={handlePrev}
                >
                  Prev
                </button>
              )}
              <button
                disabled={isNextDisabled}
                className="border rounded shadow-xl shadow-green-400/20 border-green-500 p-1 disabled:invisible"
                onClick={handleNext}
              >
                {isSubmitting ? "Building ..." : "Next"}
              </button>
            </div>
          </div>
        );
      }}
    </FormikWizard>
  );
};
