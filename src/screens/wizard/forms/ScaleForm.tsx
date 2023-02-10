import React from "react";
import { ScaleField } from "../fields/ScaleField";
import type { StepProps } from "../types";

export const ScaleForm: React.FC<StepProps> = (props) => {
  return (
    <div className="text-center h-full my-7">
      <div className="text-9xl mb-6"> 📏 </div>
      <div className="font-light text-4xl"> Lets talk about scale</div>
      <div className="font-light text-md mt-4 text-justify">
        Arkitekt can be deployed in a variety of different scales, depending on
        how many users you expect to have, and how much data you expect to
        store. The scale of your deployment will determine how much hardware you
        need to run Arkitekt.
      </div>
      <ScaleField name="scale" />
    </div>
  );
};
