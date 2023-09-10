import { ErrorMessage, Field } from "formik";
import React from "react";
import type { StepProps } from "../types";
import { ErrorDisplay } from "../../../components/Error";
import { useChannel } from "../../../channel/channel-context";
export const ChannelGreeting: React.FC<StepProps> = (props) => {
  const { name, description } = useChannel();
  return (
    <div className="items-center h-full my-7">
      <div className="font-light text-7xl">For your info</div>
      <div className="font-light text-3xl mt-4">
        Your are installing Arkitekt via the channel {name}
      </div>
      <div className="mb-2 text-justify mt-4 max-w-xl"></div>
      {description}
    </div>
  );
};
