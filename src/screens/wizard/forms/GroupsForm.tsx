import { ErrorMessage, Field, FieldArray, useFormikContext } from "formik";
import React from "react";
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
      <div className="my-4 text-center">
        <FieldArray
          name="groups"
          render={(arrayHelpers) => (
            <>
              <div className="flex flex-row gap-2">
                {values?.groups?.map((friend, index) => (
                  <div
                    key={index}
                    className="relative flex-1 border border-1 border-primary-200 "
                  >
                    <div className="font-light text-xl my-1">Group name</div>
                    <Field
                      name={`groups.${index}.name`}
                      className="text-black"
                    />
                    <div className="font-light text-xl my-1">Description</div>
                    <Field
                      name={`groups.${index}.description`}
                      className="text-black"
                    />
                    <button
                      type="button"
                      className="absolute top-0 right-0"
                      onClick={() => arrayHelpers.remove(index)} // remove a friend from the list
                    >
                      X
                    </button>
                  </div>
                ))}
              </div>
              <button
                type="button"
                onClick={() => arrayHelpers.push({ name: "", description: "" })}
              >
                {/* show this when user has removed all friends from the list */}
                Add a group
              </button>
            </>
          )}
        />
      </div>
    </div>
  );
};
