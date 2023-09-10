import { useFormikContext } from "formik";
import React from "react";
import { FileField } from "../fields/FileField";
import { SetupValues } from "../Setup";
import type { StepProps } from "../types";

export const AppStorage: React.FC<StepProps> = (props) => {
  const { values } = useFormikContext<SetupValues>();

  return (
    <div className="text-center h-full my-7">
      <div className="font-light text-3xl mt-4">Choose your apps folder</div>
      <div>
        <FileField name="app_path" />
      </div>
      <div className="text-center mt-6">
        This folder will be used to store all of the data and the configuration
        of the platform. You should not have anything else stored in this
        folder, as it will be used to store the data of the platform.
        {values.services.find((s) => s.name == "mikro") && (
          <p className="p-5 font-light">
            ̛ Your setup includes mikro which is hosting all of your data files.
            Make sure you have enough space for the data files in this folder.
          </p>
        )}
      </div>
    </div>
  );
};
