import React from "react";
import { ChannelSelectionField } from "../fields/ChannelSelectionField";
import type { StepProps } from "../types";

export const ChannelForm: React.FC<StepProps> = (props) => {
  return (
    <div className="h-full w-full my-7 flex flex-col">
      <div className="font-light text-7xl"> Hard things first!</div>
      <div className="font-light text-2xl mt-4">
        Lets choose your Arkitekt variant
      </div>
      <div className="mb-2 text-justify mt-4 max-w-xl">
        The Arkitekt distribution comes in different flavours, that are
        optimized to fit your willingness to live on the bleeding edge or to use
        a more well tested version of Arkitekt as well as an Arkitekt version
        frozen in time. Below find the currently available versions of Arkitekt.
      </div>
      <div className="max-w-xl">
        <ChannelSelectionField name="channel" />
      </div>
    </div>
  );
};
