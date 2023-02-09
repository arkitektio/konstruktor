import React from "react";
import { AppSelectionField } from "../fields/AppSelectionField";
import type { StepProps } from "../types";

export const AppSelection: React.FC<StepProps> = (props) => {
  return (
    <div className="flex flex-grow flex-col">
      <div className="font-light text-3xl mt-4">
        Which apps should be automatically configured to be able connect to the
        platform
      </div>
      <div className="text-center mt-6">
        Arkitekt is build around apps that provide specific functionality. But
        because we care about data safety, apps need to fully authenticate
        themselves with the platform and negotiate access rights. You can
        already enable some of the standard apps that Arkitekt provides here, so
        that there is no need for configuration.
      </div>
      <div className="mt-3">
        <AppSelectionField name="apps" />
      </div>
    </div>
  );
};
