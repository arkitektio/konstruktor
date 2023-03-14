import React from "react";
import { ServiceSelectionField } from "../fields/ServiceSelectionField";
import type { StepProps } from "../types";

export const ServiceSelection: React.FC<StepProps> = (props) => {
  return (
    <div className="flex flex-grow flex-col text-center my-7 overflow-y-scroll">
      <div className="font-light text-3xl mt-4">
        Which services do you want to install
      </div>
      <div className="text-center mt-6">
        Arkitekt is build around services that provide specific functionality.
        Depending on your usecase you might want not install all of them.
        <br />
        <br /> If you are unsure, just leave the selected, selected.
      </div>
      <div className="mt-5 ">
        <ServiceSelectionField name="services" />
      </div>
    </div>
  );
};
