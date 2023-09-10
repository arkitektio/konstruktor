import { ErrorMessage, Field, FieldArray, useFormikContext } from "formik";
import React from "react";
import { Hover } from "../../../layout/Hover";
import { MultiSelectReduceField } from "../fields/MultiSelectField";
import { SetupValues } from "../Setup";
import type { StepProps } from "../types";
import { Card } from "../../../components/ui/card";

export type User = {
  username: string;
  name?: string;
  password: string;
  email?: string;
  groups: string[];
};

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
    <div className="text-center w-full">
      <div className="font-light text-3xl mt-4">
        Lets set up some initial Users
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
        name="users"
        render={(arrayHelpers) => (
          <>
            <Hover className="my-4 flex justify-center flex-row gap-2 flex-wrap">
              {values?.users?.map((friend, index) => (
                <Card
                  key={index}
                  className="group relative hovercard flex-initial border border-1 border-slate-200 p-3"
                >
                  <div className="font-light text-xl my-1">Groups</div>
                  <div className="justify-center">
                    {values.groups && (
                      <MultiSelectReduceField
                        name={`users.${index}.groups`}
                        options={values.groups}
                        unique={"name"}
                        label={"name"}
                      />
                    )}
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
                  <div className="font-light text-xl my-1">Email</div>
                  <Field
                    name={`users.${index}.email`}
                    type="email"
                    className="text-black p-1 rounded"
                  />
                  <ErrorMessage name={`users.${index}.email`}>
                    {(msg) => (
                      <div className="text-center border border-red-400 mt-1 rounded p-1 text-red-300 my-auto">
                        {msg}
                      </div>
                    )}
                  </ErrorMessage>
                  <div className="font-light text-xs text-md my-1">
                    The user email to reach you
                  </div>
                  <div className="font-light text-xl my-1">Username</div>
                  <Field
                    name={`users.${index}.username`}
                    className="text-black p-1 rounded"
                  />
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
                  <Field
                    name={`users.${index}.password`}
                    type="password"
                    className="text-black p1 rounded"
                  />
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
            <button
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
              className="bg-slate-800 text-white rounded-md p-2 hover:bg-slate-700"
            >
              {/* show this when user has removed all friends from the list */}
              Add a user
            </button>
          </>
        )}
      />
    </div>
  );
};
