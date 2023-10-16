import { ErrorMessage, Field, FieldArray, useFormikContext } from "formik";
import React from "react";
import { Hover } from "../../../layout/Hover";
import type { StepProps } from "../types";
import { SetupValues } from "../../../repo/repo-context";
import { Card } from "../../../components/ui/card";
import { Button } from "../../../components/ui/button";
import { ErrorDisplay } from "../../../components/Error";

export const availablePermissions = [
  "admin",
  "read",
  "write",
  "delete",
  "create",
  "update",
];

export const GroupsForm: React.FC<StepProps> = ({ errors }) => {
  const { values } = useFormikContext<SetupValues>();

  return (
    <div className="h-full w-full my-7 flex flex-col">
      <div className="font-light text-7xl"> Groups!</div>
      <div className="font-light text-2xl mt-4">
        Lets set up some initial Groups
      </div>
      <div className="mb-2 text-justify mt-4 max-w-xl">
        Arkitekt uses groups to manage collections of users, e.g Teams or
        Organizations. Groups can be used to assign permissions to use specific
        apps and resources.
        <br />
        <div className="font-light text-xs  mt-2">
          As of now users can only access data within their own groups, but you
          can change this through the admin interface.
        </div>
      </div>
      <div className="max-w-xl">
        <FieldArray
          name="groups"
          render={(arrayHelpers) => (
            <>
              <Hover className="my-4 flex flex-row gap-2 flex-wrap">
                {values?.groups?.map((friend, index) => (
                  <Card
                    key={index}
                    className="group relative hovercard flex-initial  p-3"
                  >
                    <div className="font-light text-md my-1">Group name</div>
                    <Field
                      name={`groups.${index}.name`}
                      autoComplete="off"
                      spellCheck="false"
                      className="text-black p-1 rounded"
                    />
                    <ErrorDisplay name={`groups.${index}.name`} className="mt-2">
                      {(msg) => (
                        <div className="text-center border border-red-400 mt-1 rounded p-1 text-red-300 my-auto">
                          {msg}
                        </div>
                      )}
                    </ErrorDisplay>
                    <div className="font-light text-xs text-md my-2">
                      A name for this group
                    </div>
                    <div className="font-light text-md my-1">Description</div>
                    <Field
                      name={`groups.${index}.description`}
                      className="text-black p-1 rounded"
                    />
                    <ErrorDisplay name={`groups.${index}.description`} className="mt-2">
                      {(msg) => (
                        <div className="text-center border border-red-400 mt-1 rounded p-1 text-red-300 my-auto">
                          {msg}
                        </div>
                      )}
                    </ErrorDisplay>
                    <div className="font-light text-xs text-md my-2">
                      Briefly describe this group
                    </div>
                    <button
                      type="button"
                      className="group-hover:visible invisible absolute top-0 right-0 transform translate-x-1/2 -translate-y-1/2 bg-red-500 rounded-full h-6 w-6 flex items-center justify-center text-white"
                      onClick={() => arrayHelpers.remove(index)} // remove a friend from the list
                    >
                      x
                    </button>
                  </Card>
                ))}
              </Hover>
              <Button
                type="button"
                onClick={() => arrayHelpers.push({ name: "", description: "" })}
              >
                {/* show this when user has removed all friends from the list */}
                Add a group
              </Button>
            </>
          )}
        />
      </div>
    </div>
  );
};
