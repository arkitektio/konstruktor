import { ErrorMessage, Field } from "formik";
import React from "react";
import type { StepProps } from "../types";
import { ErrorDisplay } from "../../../components/Error";

export const AdminUserForm: React.FC<StepProps> = ({ errors }) => {
  return (
    <div className="h-full w-full my-7 flex flex-col">
      <div className="font-light text-7xl"> Attention !</div>
      <div className="font-light text-2xl mt-4">
        Your Arkitekt instance needs an Admin 🦸‍♂️
      </div>
      <div className="mb-2 text-justify mt-4 max-w-xl">
        Administrators are power users that can manage the Arkitekt instance.
        and have access to all data. You can add more administrators later. This
        admin user will be created during the setup process and will be able to
        access every service.
      </div>
      <div className="max-w-xl">
        <div className="my-auto">
          <div className="my-auto">
            <div className="font-light text-xl my-1">Username</div>
            <Field
              name="adminUsername"
              className="text-center border border-gray-400 rounded p-2 text-black"
            />
            <ErrorDisplay name="adminUsername">
              {(msg) => (
                <div className="text-center border border-red-400 mt-1 rounded p-1 text-red-300 my-auto">
                  {msg}
                </div>
              )}
            </ErrorDisplay>
            <div className="font-light text-xs my-1">the login name</div>
          </div>
          <div className="font-light text-xl my-1 mt-3">Password</div>
          <Field
            type="password"
            name="adminPassword"
            className="text-center border border-gray-400 rounded p-2 text-black"
          />
          <ErrorDisplay name="adminPassword">
            {(msg) => (
              <div className="text-center border border-red-400 mt-1 rounded p-1 text-red-300 my-auto">
                {msg}
              </div>
            )}
          </ErrorDisplay>
          <div className="font-light text-xs my-1">
            A password for the admin user
          </div>
        </div>
      </div>
    </div>
  );
};
