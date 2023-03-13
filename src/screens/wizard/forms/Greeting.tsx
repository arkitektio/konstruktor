import { ErrorMessage, Field } from "formik";
import React from "react";
import type { StepProps } from "../types";
export const Greeting: React.FC<StepProps> = (props) => {
  return (
    <div className="text-center h-full my-7">
      <div className="font-light text-7xl"> Hello!</div>
      <div className="font-light text-3xl mt-4">
        Lets install the framework together
      </div>
      <div className="mb-2 text-justify mt-4">
        First things first: Lets think about naming your install. With
        Konstruktor you can install multiple versions of the Arkitekt platform,
        with multiple configurations. You can imaging having an experimental
        setup with different functionality and one production setup with stable
        configuration. So lets name your install (e.g. myexperimentalsetup) Apps
        thats want to connect to the platform will see this name!
      </div>
      <div className="mt-6">
        <Field
          type="input"
          name="name"
          className="border rounded border-gray-500 text-center p-2 text-black"
          placeholder="Your Installs Name"
        />
        <ErrorMessage name="name" />
      </div>
    </div>
  );
};
