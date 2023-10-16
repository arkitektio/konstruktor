import React from "react";
import type { StepProps } from "../types";

export const Done: React.FC<StepProps> = (props) => {
  return (
    <div className="h-full w-full my-7 flex flex-col">
      <div className="font-light  text-7xl"> Done!  👏</div>
      <div className="font-light text-xl mt-4 text-justify">
        Thats all of the information we need from you! You can review your
        choices or click next to install your app.
      </div>
    </div>
  );
};
