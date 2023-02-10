import React from "react";
import { ScaleField } from "../fields/ScaleField";
import type { StepProps } from "../types";

export const Done: React.FC<StepProps> = (props) => {
  return (
    <div className="text-center h-full my-7">
      <div className="text-9xl mb-6"> ☕ </div>
      <div className="font-light text-4xl"> Lets tak about scale</div>
      <div className="font-light text-xl mt-4 text-justify">
        Please make sure your computer is connected to the internet and all
        unnecessary apps are closed. If you click next the app will be
        installed. Get a coffee ready...
      </div>
      <ScaleField name="scale" />
    </div>
  );
};
