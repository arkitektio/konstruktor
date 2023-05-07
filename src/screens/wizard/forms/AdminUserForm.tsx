import { ErrorMessage, Field } from "formik";
import React from "react";
import type { StepProps } from "../types";

export const AdminUserForm: React.FC<StepProps> = ({ errors }) => {
  return (
    <div className="w-80 text-center p-9 mx-auto">
      <div className="font-light text-9xl text-center ">🦸‍♂️</div>
      <div className="mt-5  p-2 my-auto">
        Again NEVER login with this user account in one of your apps! This
        account is only for managing and accessing the admin panels within
        arkitekt.
      </div>
      <div className="my-auto text-center">
        <div className="my-auto">
          <div className="font-light text-xl my-1">Username</div>
          <Field
            name="admin_username"
            className="text-center border border-gray-400 rounded p-2 text-black"
          />
          <ErrorMessage name="admin_username">
            {(msg) => (
              <div className="text-center border border-red-400 mt-1 rounded p-1 text-red-300 my-auto">
                {msg}
              </div>
            )}
          </ErrorMessage>
          <div className="font-light text-xs my-1">the login name</div>
        </div>
        <div className="font-light text-xl my-1 mt-3">Email address</div>
        <Field
          type="email"
          name="admin_email"
          className="text-center border border-gray-400 rounded p-2 text-black"
        />
        <ErrorMessage name="admin_email">
          {(msg) => (
            <div className="text-center border border-red-400 mt-1 rounded p-1 text-red-300 my-auto">
              {msg}
            </div>
          )}
        </ErrorMessage>
        <div className="font-light text-xs my-1">
          For contacting you in an emergency
        </div>
        <div className="font-light text-xl my-1 mt-3">Password</div>
        <Field
          type="password"
          name="admin_password"
          className="text-center border border-gray-400 rounded p-2 text-black"
        />
        <ErrorMessage name="admin_password">
          {(msg) => (
            <div className="text-center border border-red-400 mt-1 rounded p-1 text-red-300 my-auto">
              {msg}
            </div>
          )}
        </ErrorMessage>
        <div className="font-light text-xs my-1">
          A password for the admin user
        </div>
      </div>
    </div>
  );
};
