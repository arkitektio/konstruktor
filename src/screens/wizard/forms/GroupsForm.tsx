import { ErrorMessage, Field, FieldArray, useFormikContext } from "formik";
import React from "react";
import { Hover } from "../../../layout/Hover";
import { SetupValues } from "../Setup";
import type { StepProps } from "../types";

export type Group = {
  name: string;
  description: string;
};

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
    <div className="text-center w-full">
      <div className="font-light text-3xl mt-4">
        Lets set up some initial Groups
      </div>
      <div className="text-center mt-6">
        Arkitekt uses groups to manage collections of users, e.g Teams or
        Organizations Groups can be used to manage access to apps, and to manage
        the users within the group.
        <br />
        By default users can only access data within their own groups, but you
        can change this through the admin interface.
      </div>
      <FieldArray
        name="groups"
        render={(arrayHelpers) => (
          <>
            <Hover className="my-4 flex justify-center flex-row gap-2 flex-wrap">
              {values?.groups?.map((friend, index) => (
                <div
                  key={index}
                  className="group relative hovercard flex-initial border border-1 border-slate-200 p-3"
                >
                  <div className="font-light text-md my-1">Group name</div>
                  <Field name={`groups.${index}.name`} className="text-black" />
                  <ErrorMessage name={`groups.${index}.name`}>
                    {(msg) => (
                      <div className="text-center border border-red-400 mt-1 rounded p-1 text-red-300 my-auto">
                        {msg}
                      </div>
                    )}
                  </ErrorMessage>
                  <div className="font-light text-md my-1">Description</div>
                  <Field
                    name={`groups.${index}.description`}
                    className="text-black"
                  />
                  <ErrorMessage name={`groups.${index}.description`}>
                    {(msg) => (
                      <div className="text-center border border-red-400 mt-1 rounded p-1 text-red-300 my-auto">
                        {msg}
                      </div>
                    )}
                  </ErrorMessage>
                  <button
                    type="button"
                    className="group-hover:visible invisible absolute top-0 right-0 transform translate-x-1/2 -translate-y-1/2 bg-red-500 rounded-full h-6 w-6 flex items-center justify-center text-white"
                    onClick={() => arrayHelpers.remove(index)} // remove a friend from the list
                  >
                    x
                  </button>
                </div>
              ))}
            </Hover>
            <button
              type="button"
              onClick={() => arrayHelpers.push({ name: "", description: "" })}
              className="bg-slate-800 text-white rounded-md p-2 hover:bg-slate-700"
            >
              {/* show this when user has removed all friends from the list */}
              Add a group
            </button>
          </>
        )}
      />
    </div>
  );
};
