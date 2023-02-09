import { ErrorMessage, Field, FieldArray, useFormikContext } from "formik";
import React from "react";
import {
  MultiSelectField,
  MultiSelectReduceField,
} from "../fields/MultiSelectField";
import { MultiSelect } from "../MultiSelect";
import { SetupValues } from "../Setup";
import type { StepProps } from "../types";

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
      <div className="my-4 text-center">
        <FieldArray
          name="users"
          render={(arrayHelpers) => (
            <>
              <div className="flex flex-row gap-2">
                {values?.users?.map((friend, index) => (
                  <div
                    key={index}
                    className="relative flex-1 border border-1 border-primary-200 p-3 "
                  >
                    <div className="font-light text-xl my-1">Groups</div>
                    <div className="justify-center flex">
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
                      {(msg) => <div className="bg-red-700">{msg}</div>}
                    </ErrorMessage>
                    <div className="font-light text-xl my-1">Email</div>
                    <Field
                      name={`users.${index}.email`}
                      type="email"
                      className="text-black"
                    />
                    <ErrorMessage name={`users.${index}.email`}>
                      {(msg) => (
                        <div className="bg-red-700">Must be a valid Email</div>
                      )}
                    </ErrorMessage>
                    <div className="font-light text-xl my-1">Username</div>
                    <Field
                      name={`users.${index}.username`}
                      className="text-black"
                    />
                    <ErrorMessage name={`users.${index}.username`}>
                      {(msg) => (
                        <div className="bg-red-700">
                          Must be a valid username
                        </div>
                      )}
                    </ErrorMessage>
                    <div className="font-light text-xl my-1">Password</div>
                    <Field
                      name={`users.${index}.password`}
                      type="password"
                      className="text-black"
                    />
                    <ErrorMessage name={`users.${index}.password`}>
                      {(msg) => (
                        <div className="bg-red-700">
                          Must be a valid password
                        </div>
                      )}
                    </ErrorMessage>
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
              </button>
            </>
          )}
        />
      </div>
    </div>
  );
};
