import { useField, useFormikContext } from "formik";
import { Card } from "../../../components/ui/card";
import { SetupValues } from "../Setup";
import { Badge } from "../../../components/ui/badge";


export type AvailableForms = "greeting" | "check_docker" | "service_selection" | "app_storage" | "app_selection" | "attention_superuser" | "done" | "admin_user" | "groups" | "users" | "scale" | "channel";



export type Feature = {
  name: string;
  description: string;
  long: string;
}



export type Channel = {
  title: string
  name: string;
  builder: string;
  logo: string;
  long: string;
  experimental: boolean;
  description: string;
  features: Feature[];
  forms: AvailableForms[];
  defaults: Partial<SetupValues>;
};

export const availableChannels: Channel[] = [
  {
    name: "paper",
    title: "Paper",
    experimental: false,
    logo: "https://python.doctor/images/django-python.png",
    long: "The paper deployment to reproduce the paper. Comes with a preconfigured set of apps and services to reproduce the paper",
    description: "The original paper deployment",
    features: [
      {
        name: "Easy",
        description: "Easy to use",
        long: "This deployment is easy to use",
      }
    ],
    builder: "jhnnsrs/arkitekt_paper_builder",
    forms: ["admin_user", "users", "groups"],
    defaults: {
      name: "mydeployment",
      admin_username: "",
      admin_password: "",
      admin_email: "",
    },
  },
  {
    name: "next",
    title: "next",
    experimental: true,
    logo: "https://python.doctor/images/django-python.png",
    long: "The next deployment contains breaking changes and is not yet stable. It does however give an impression of the future of Arkitekt",
    description: "The next deployment",
    features: [
      {
        name: "Easy",
        description: "Easy to use",
        long: "This deployment is easy to use",
      }
    ],
    builder: "jhnnsrs/arkitekt_paper_builder",
    forms: ["admin_user", "users", "groups"],
    defaults: {
      name: "mydeployment",
      admin_username: "",
      admin_password: "",
      admin_email: "",
    },
  },
];






export const ChannelSelectionField = ({ ...props }: any) => {
  const [field, meta, helpers] = useField<string>(props);

  const toggleValue = async (app: Channel) => {
    helpers.setValue(app.name);
    console.log(field.value);
  };

  return (
    <>
      <div className="grid grid-cols-2 @xl:grid-cols-2 gap-2 mt-2 ">
        {availableChannels.map((app, i) => {

          return (
            <Card
              className={`cursor-pointer relative ${
                field.value === app.name
                  ? " border-slate-200 "
                  : "shadow-primary-300/20 border-slate-400 opacity-40 "
              }`}
              key={i}
              onClick={() => toggleValue(app)}
            >
              <div className="items-start p-3">
                <div className="flex flex-row justify-between">
                  <img
                    className="text-sm text-start h-20"
                    src={app.logo}
                  />
                </div>
                <div className="font-bold text-start flex-row flex justify-between">
                  <div className="my-auto">{app.title}</div>
                  
                </div>
                <div className="font-light  text-start">{app.description}</div>
                <div className="text-sm  text-start mt-1">{app.long}</div>
                
              </div>
              {app.experimental && (
                    <Badge className="absolute top-0 left-3  translate-y-[-50%]">
                      Experimental
                    </Badge>
                  )}
            </Card>
          );
        })}
      </div>

      {meta.touched && meta.error ? (
        <div className="error">{meta.error}</div>
      ) : null}
    </>
  );
};
