import React from "react";
import type { StepProps } from "../types";

export const Done: React.FC<StepProps> = (props) => {
  return (
    <div className="text-center h-full w-full flex items-center flex-col my-7">
      <div className="text-9xl mb-6"> 👏 </div>
      <div className="font-light text-4xl"> Done!</div>
      <div className="font-light text-xl mt-4 text-justify">
        Thats all of the information we need from you! You can review your
        choices or click next to install your app.
      </div>
    </div>
  );
};
