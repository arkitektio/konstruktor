import { useField } from "formik";
import { Badge } from "../../../components/ui/badge";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "../../../components/ui/card";
import { Channel, useRepo } from "../../../repo/repo-context";
import { ScrollArea } from "../../../components/ui/scroll-area";

export const ChannelSelectionField = ({ ...props }: any) => {
  const [field, meta, helpers] = useField<string | undefined>(props);
  const { channels, errors } = useRepo();

  const toggleValue = async (channel: Channel) => {
    helpers.setValue(channel.name);
    console.log(field.value);
  };

  return (
    <>
      <div className="flex flex-row w-full gap-2 mt-2 ">
        {channels.map((channel, i) => {
          return (
            <Card
              className={`cursor-pointer relative ${
                field.value === channel.name
                  ? " border-slate-200 "
                  : "shadow-primary-300/20 border-slate-400 opacity-40 "
              }`}
              key={i}
              onClick={() => toggleValue(channel)}
            >
              <div className="items-start p-3">
                <div className="flex flex-row justify-between">
                  <img className="text-sm text-start aspect-square	 w-full" src={channel.logo} />
                </div>
                <div className="font-bold text-start flex-row flex justify-between">
                  <div className="my-auto">{channel.title}</div>
                </div>
                <div className="font-light  text-start">
                  {channel.description}
                </div>
                <div className="text-sm  text-start mt-1">{channel.long}</div>
              </div>
              {channel.experimental && (
                <Badge className="absolute top-0 left-3  translate-y-[-50%]">
                  Experimental
                </Badge>
              )}
            </Card>
          );
        })}
      </div>
      {errors.length > 0 && <div className="grid grid-cols-4 @xl:grid-cols-4 gap-2 mt-2 ">
        {errors.map((e) => <Card className="bg-destructive"><CardHeader><CardTitle>Error</CardTitle><CardDescription>The repo {e.repo} is faulty</CardDescription></CardHeader>
        <CardContent><ScrollArea>{e.errors.map((e) => <div>{e.path} {e.message}</div>)}</ScrollArea></CardContent>
        <CardFooter>Tell these errors to the Repo Owner </CardFooter></Card>)}
      </div>}

      {meta.touched && meta.error ? (
        <div className="error">{meta.error}</div>
      ) : null}
    </>
  );
};
