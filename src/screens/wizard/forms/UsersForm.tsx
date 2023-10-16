import { ErrorMessage, Field, FieldArray, useFormikContext } from "formik";
import React from "react";
import { Hover } from "../../../layout/Hover";
import { MultiSelectReduceField } from "../fields/MultiSelectField";
import type { StepProps } from "../types";
import { Card } from "../../../components/ui/card";
import { SetupValues } from "../../../repo/repo-context";
import { Select } from "../../../components/ui/select";
import { OptionField } from "../fields/OptionField";
import { UIField } from "../../../components/FormikInput";
import { Button } from "../../../components/ui/button";
import { MultiOptionField } from "../fields/MultiOptionField";

export const availablePermissions = [
  "admin",
  "read",
  "write",
  "delete",
  "create",
  "update",
];

export const UsersForm: React.FC<StepProps> = ({ errors }) => {
  const { values } = useFormikContext<SetupValues>();

  return (
    <div className="h-full w-full my-7 flex flex-col">
      <div className="font-light text-7xl"> Users!</div>
      <div className="font-light text-3xl mt-4">
        Lets set up some initial Users
      </div>
      <div className="mb-2 text-justify mt-4 max-w-xl">
        Arkitekt uses groups to manage collections of users, e.g Teams or
        Organizations Groups can be used to manage access to apps, and to manage
        the users within the group.
        <br />
        <div className="font-light text-xs  mt-2">
        By default users can only access data within their own groups, but you
        can change this through the admin interface.
        </div>
        
      </div>
      <div className="max-w-xl">
      <FieldArray
        name="users"
        render={(arrayHelpers) => (
          <>
            <Hover className="my-4 flex flex-row gap-2 flex-wrap">
              {values?.users?.map((friend, index) => (
                <Card
                  key={index}
                  className="group relative hovercard flex-initial  p-3"
                >
                  <div className="font-light text-xl my-1">Groups</div>
                  <div className="justify-center">
                    <MultiOptionField
                      name={`users.${index}.groups`}
                      options={values.groups.map((g) => ({
                        value: g.name,
                        label: g.name,
                      }))}
                    />
                  </div>

                  <ErrorMessage name={`users.${index}.groups`}>
                    {(msg) => (
                      <div className="text-center border border-red-400 mt-1 rounded p-1 text-red-300 my-auto">
                        {msg}
                      </div>
                    )}
                  </ErrorMessage>
                  <div className="font-light text-xs text-md my-1">
                    Groups this user belongs to
                  </div>
                  <div className="font-light text-xl my-1">Username</div>
                  <UIField name={`users.${index}.username`} />
                  <ErrorMessage name={`users.${index}.username`}>
                    {(msg) => (
                      <div className="text-center border border-red-400 mt-1 rounded p-1 text-red-300 my-auto">
                        {msg}
                      </div>
                    )}
                  </ErrorMessage>
                  <div className="font-light text-xs text-md my-1">
                    Your unique username
                  </div>
                  <div className="font-light text-xl my-1">Password</div>
                  <UIField name={`users.${index}.password`} type="password" />
                  <ErrorMessage name={`users.${index}.password`}>
                    {(msg) => (
                      <div className="text-center border border-red-400 mt-1 rounded p-1 text-red-300 my-auto">
                        {msg}
                      </div>
                    )}
                  </ErrorMessage>
                  <div className="font-light text-xs text-md my-1">
                    Password for this user
                  </div>
                  <button
                    type="button"
                    className="group-hover:visible invisible absolute top-0 right-0 transform translate-x-1/2 -translate-y-1/2 bg-red-500 rounded-full h-6 w-6 flex items-center justify-center text-white"
                    onClick={() => arrayHelpers.remove(index)} // remove a friend from the list
                  >
                    X
                  </button>
                </Card>
              ))}
            </Hover>
            <Button
              type="button"
              onClick={() =>
                arrayHelpers.push({
                  username: "",
                  name: "",
                  first_time_password: "",
                  email: "",
                  groups: [],
                })
              }
            >
              {/* show this when user has removed all friends from the list */}
              Add a user
            </Button>
          </>
        )}
      />
      </div>
    </div>
  );
};
