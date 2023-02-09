import { ErrorMessage, Field } from "formik";
import React from "react";
import type { StepProps } from "../types";

export const AdminUserForm: React.FC<StepProps> = ({ errors }) => {
  return (
    <div className="text-center w-full">
      <div className="font-light text-3xl mt-4">About the Superuser...</div>
      <div className="font-bold mt-5 text-red-200 border-red-200 border p-1">
        NEVER login with this user account in one of your apps! This account is
        only for managing and accessing the admin panels within arkitekt.
      </div>
      <div className="my-4 text-center">
        <div className="my-4">
          <div className="font-light text-xl my-1">Username</div>
          <Field
            name="admin_username"
            className="text-center border border-gray-400 rounded p-2 text-black"
          />
          <ErrorMessage name="admin_username">
            {(msg) => <div className="bg-red-700">{msg}</div>}
          </ErrorMessage>
        </div>
        <div className="font-light text-xl my-1">Email address</div>
        <Field
          type="email"
          name="admin_email"
          className="text-center border border-gray-400 rounded p-2 text-black"
        />
        <ErrorMessage name="admin_email">
          {(msg) => <div className="bg-red-700">{msg}</div>}
        </ErrorMessage>
        <div className="font-light text-xl my-1">Password</div>
        <Field
          type="password"
          name="admin_password"
          className="text-center border border-gray-400 rounded p-2 text-black"
        />
        <ErrorMessage name="admin_password">
          {(msg) => <div className="bg-red-700">{msg}</div>}
        </ErrorMessage>
      </div>
    </div>
  );
};
