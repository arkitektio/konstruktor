import React from "react";
import { FileField } from "../fields/FileField";
import type { StepProps } from "../types";

export const AppStorage: React.FC<StepProps> = (props) => {
  return (
    <div className="h-full my-7">
      <div className="font-light text-3xl mt-4">Choose your apps folder</div>
      <div>
        <FileField name="path" />
      </div>
      <div className="text-center mt-6">
        This folder will be used to store all of the data and the configuration
        of the platform. You should not have anything else stored in this
        folder, as it will be used to store the data of the platform.
      </div>
    </div>
  );
};
