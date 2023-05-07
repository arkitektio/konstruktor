import React from "react";
import { ServiceSelectionField } from "../fields/ServiceSelectionField";
import type { StepProps } from "../types";

export const ServiceSelection: React.FC<StepProps> = (props) => {
  return (
    <div className="h-full items-center flex flex-col">
      <div className="font-light text-3xl flex-initial">
        Which services do you want to install
      </div>
      <div className="text-center flex-initial mt-2">
        Arkitekt is build around services that provide specific functionality.
        Depending on your usecase you might want not install all of them.
        <br />
        <br /> If you are unsure, just leave the selected, selected.
      </div>
      <ServiceSelectionField name="services" />
    </div>
  );
};
