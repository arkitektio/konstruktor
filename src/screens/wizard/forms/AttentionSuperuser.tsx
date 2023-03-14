import { Field } from "formik";
import React from "react";
import type { StepProps } from "../types";

export const AttentionSuperuser: React.FC<StepProps> = (props) => {
  return (
    <div className="my-7 p-10">
      <div className="font-light text-9xl text-center ">🔥</div>
      <div className="font-light text-3xl mt-8 text-center ">
        About the superuser!
      </div>
      <div className="mt-3 my-auto text-center ">
        <div className="text-justify mt-4 text-center ">
          This platform needs a superuser. And you will get to decide who that
          is! A superuser is a <b>complete</b> administator of the platform, and
          has full access to all the data. They will be be able to create new
          users, and manage all the users. Also they will have complete access
          to every users private data.
        </div>
        <div className="font-light text-4xl mt-5 text-center ">
          Make sure your password is safe!
        </div>
        <div className="font-light text-xl mt-2 text-center ">
          Do <b>not</b> login with this account when accessing your data!
        </div>
        <div className="mt-6">Understood?</div>
        <div className="w-full flex justify-center mt-2">
          <div className="mt-2 border border-primary-200 w-11 h-11 my-auto rounded border border-slate-200 cursor-pointer">
            <Field
              type="checkbox"
              name="attention"
              className="w-10 h-10 text-primary-600 bg-slate-100 border-slate-100 rounded ring-primary-500 cursor-pointer ring-2 dark:bg-gray-100 dark:border-gray-100"
            />
          </div>
        </div>
      </div>
    </div>
  );
};
