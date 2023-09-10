import { FormikHelpers, FormikValues } from "formik";
import { FormikWizard, RenderProps, Step } from "formik-wizard-form";
import { Link, useNavigate, useParams } from "react-router-dom";
import * as Yup from "yup";
import { ChannelProvider } from "../../channel/channel-provider";
import { Button } from "../../components/ui/button";
import { Page } from "../../layout/Page";
import {
  AvailableForms,
  Channel,
  Group,
  SetupValues,
  User,
  useRepo,
} from "../../repo/repo-context";
import { useStorage } from "../../storage/storage-context";
import { AdminUserForm } from "./forms/AdminUserForm";
import { AppSelection } from "./forms/AppSelection";
import { AttentionSuperuser } from "./forms/AttentionSuperuser";
import { Done } from "./forms/Done";
import { ServiceSelection } from "./forms/ServiceSelection";
import { UsersForm } from "./forms/UsersForm";
import { GroupsForm } from "./forms/GroupsForm";
import { ChannelGreeting } from "./forms/ChannelGreeting";
import { useState } from "react";
import { useCommand, useLazyCommand } from "../../hooks/useCommand";
import {
  Dialog,
  DialogContent,
  DialogDescription,
} from "../../components/ui/dialog";
import { DialogTitle } from "@radix-ui/react-dialog";
import { ScrollArea } from "@radix-ui/react-scroll-area";
import { writeText, readText } from "@tauri-apps/api/clipboard";
export const debugUser = {
  name: "debug",
  username: "debug",
  password: "debug",
  email: "debug@debug.com",
  groups: ["myteam"],
};

export type ConditionalStep = Step & {
  name: AvailableForms;
};

export const allSteps: ConditionalStep[] = [
  {
    name: "greeting",
    component: ChannelGreeting,
  },
  {
    name: "service_selection",
    component: ServiceSelection,
    validationSchema: Yup.object().shape({
      services: Yup.array().required("Desired Modules Required"),
    }),
  },
  {
    name: "app_selection",
    component: AppSelection,
    validationSchema: Yup.object().shape({
      apps: Yup.array().required("Desired Modules Required"),
    }),
  },
  {
    name: "attention_superuser",
    component: AttentionSuperuser,
    validationSchema: Yup.object().shape({
      attention: Yup.bool().isTrue().required("You need to understand this"),
    }),
  },
  {
    name: "admin_user",
    component: AdminUserForm,
    validationSchema: Yup.object().shape({
      admin_username: Yup.string().required("Username is required"),
      admin_password: Yup.string().required("Password is required"),
    }),
  },
  {
    name: "groups",
    component: GroupsForm,
    validationSchema: Yup.object().shape({
      groups: Yup.array(
        Yup.object().shape({
          name: Yup.string()
            .lowercase()
            .matches(/^[a-z]+$/, "Only lowercase letters allowed")
            .required("Group name is required"),
          description: Yup.string().required(
            "A short description iss required"
          ),
        })
      )
        .required("At least one group is required")
        .test("length", "At least one group is required", (groups: Group[]) => {
          return (groups?.length || 0) > 0;
        })
        .test("unique", "Group names must be unique", (groups: Group[]) => {
          let names = groups.map((g) => g.name);
          return names.length === new Set(names).size;
        }),
    }),
  },
  {
    name: "users",
    component: UsersForm,
    validationSchema: Yup.object().shape({
      users: Yup.array(
        Yup.object().shape({
          username: Yup.string().required("Username is required"),
          password: Yup.string().required("Password is required"),
          groups: Yup.array().required("Groups are required"),
        })
      )
        .required("At least one user is required")
        .test("length", "At least one user is required", (users: User[]) => {
          return (users?.length || 0) > 0;
        })
        .test("unique", "Usernames must be unique", (users: User[]) => {
          let names = users.map((g) => g.username);
          return names.length === new Set(names).size;
        }),
    }),
  },

  {
    name: "done",
    component: Done,
  },
];

export const ChannelSetup = ({
  channel,
  name,
}: {
  channel: Channel;
  name: string;
}) => {
  const { installApp, deleteApp } = useStorage();

  const { run, logs, finished } = useLazyCommand({
    logLength: 50,
  });

  const [open, setOpen] = useState<boolean>(false);

  const [status, setStatus] = useState<string | undefined>(undefined);

  const navigate = useNavigate();

  const handleSubmit = async (
    values: Partial<SetupValues>,
    formikHelpers: FormikHelpers<FormikValues>
  ) => {
    if (values) {
      formikHelpers.setSubmitting(true);
      setOpen(true);

      let builder = "jhnnsrs/guss:prod";

      try {
        setStatus("Fetching builder ...");

        let pullResult = await run({
          program: "docker",
          args: ["pull", builder],
        });
        if (pullResult.code != 0) {
          throw Error("Error while pulling builder image");
        }

        setStatus("Creating app directory ...");

        let path = await installApp(name, channel, values);

        let pullingApp = await run({
          program: "docker",
          args: ["run", "--rm", "-v", `${path}:/app/init`, builder],
          options: {
            cwd: path,
          },
        });

        if (pullingApp.code != 0) {
          throw Error("Error while building the app folder.");
        }

        setStatus("Running ...");
      } catch (e) {
        if (e instanceof Error) {
          if (e.message) {
            setStatus(e.message);
          }
        }
        await writeText(
          "error:" +
            logs.join("\n") +
            "" +
            JSON.stringify(values) +
            JSON.stringify(channel)
        );
        await deleteApp(name);
        formikHelpers.setSubmitting(false);
      }
    }
  };

  const steps = allSteps.filter((step) => {
    return (
      channel.forms.includes(step.name as AvailableForms) ||
      step.name === "done" ||
      step.name === "greeting"
    );
  });

  return (
    <FormikWizard
      initialValues={channel.defaults}
      onSubmit={handleSubmit}
      validateOnNext
      validateOnMount
      validateOnChange
      activeStepIndex={0}
      steps={steps}
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
            <Dialog open={open} onOpenChange={setOpen}>
              <DialogContent className="bg-card">
                <DialogTitle>{status}</DialogTitle>
                {logs.length > 0 && (
                  <ScrollArea className="w-full h-max-xl bg-gray-900 p-3 rounded rounded-md">
                    {logs.map((p, i) => (
                      <div className="w-full grid grid-cols-12">
                        <div className="col-span-1">{i}</div>
                        <div className="col-span-11"> {p}</div>
                      </div>
                    ))}
                  </ScrollArea>
                )}
              </DialogContent>
            </Dialog>
            <ChannelProvider channel={channel}>
              {renderComponent()}
            </ChannelProvider>
          </Page>
        );
      }}
    </FormikWizard>
  );
};

export const InvalidRoute = (props: { children: React.ReactNode }) => {
  return (
    <Page
      buttons={
        <>
          <Button asChild>
            <Link to={"/"}>Home</Link>
          </Button>
        </>
      }
    >
      {props.children}
    </Page>
  );
};

export const ChannelSetupPage = () => {
  const { name, channel } = useParams<{
    name: string;
    channel: string;
  }>();

  const { channels } = useRepo();

  if (!channel) {
    return <InvalidRoute>Needs to provide channel</InvalidRoute>;
  }

  if (!name) {
    return <InvalidRoute>Invalid name</InvalidRoute>;
  }

  const repoChannel = channels.find((c) => c.name === channel);

  if (!repoChannel) {
    return <InvalidRoute>Invalid channel in the repo {channel}</InvalidRoute>;
  }

  try {
    return <ChannelSetup channel={repoChannel} name={name} />;
  } catch {
    return <InvalidRoute>Invalid channel</InvalidRoute>;
  }
};
