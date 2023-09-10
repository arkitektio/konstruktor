import { ErrorMessage, Field } from "formik";
import React from "react";
import type { StepProps } from "../types";
import { ErrorDisplay } from "../../../components/Error";
export const Greeting: React.FC<StepProps> = (props) => {
  return (
    <div className="items-center h-full my-7">
      <div className="font-light text-7xl"> Hello!</div>
      <div className="font-light text-3xl mt-4">
        Lets install the framework together
      </div>
      <div className="mb-2 text-justify mt-4 max-w-xl">
        First things first: Lets think about naming your install. With
        Konstruktor you can install multiple versions of the Arkitekt platform,
        with multiple configurations. You can imaging having an experimental
        setup with different functionality and one production setup with stable
        configuration. So lets name your install (e.g. myexperimentalsetup) Apps
        thats want to connect to the platform will see this name!
      </div>
      <div className="mt-6">
        <div className="flex flex-col gap-1 max-w-md">
          <label htmlFor="name" className="text-xs">
            Name your install
          </label>
          <Field
            type="input"
            name="name"
            placeholder="Your Installs Name"
            className="border rounded border-gray-500 text-center p-2 text-black"
            autoComplete="off"
            spellCheck="false"
          />
          <div className="text-xs">This is the name of your install</div>
          <ErrorDisplay name="name" />
        </div>
      </div>
    </div>
  );
};
