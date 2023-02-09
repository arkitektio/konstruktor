import { Field } from "formik";
import React from "react";
import type { StepProps } from "../types";

export const AttentionSuperuser: React.FC<StepProps> = (props) => {
  return (
    <div className="text-center  my-7">
      <div className="font-light text-9xl">🔥</div>
      <div className="font-light text-3xl mt-8">About the superuser!</div>
      <div className="mt-3 ">
        <div className="text-justify mt-4">
          This platform needs a superuser. And you will get to decide who that
          is! A superuser is a <b>complete</b> administator of the platform, and
          has full access to all the data. They will be be able to create new
          users, and manage all the users. Also they will have complete access
          to every users private data.
        </div>
        <div className="font-bold mt-5">MAKE SURE THE PASSWORD IS SAFE</div>
        <div className="mt-2">Understood?</div>
        <div className="mt-2">
          <Field
            type="checkbox"
            name="attention"
            className="w-10 h-10 text-primary-600 bg-slate-100 border-slate-100 rounded ring-primary-500  ring-2 dark:bg-gray-100 dark:border-gray-100"
          />
        </div>
      </div>
    </div>
  );
};
