import { invoke } from "@tauri-apps/api";
import React, { useEffect } from "react";
import { useCommunication } from "../../../communication/communication-context";
import { HostSelectionField } from "../fields/HostSelectionField";
import type { StepProps } from "../types";

export const AdverstisedHostsForm: React.FC<StepProps> = (props) => {
  return (
    <div className="text-center h-full my-7">
      <div className="font-light text-3xl mt-4">Bear with us...</div>
      <div className="text-center mt-6">
        As Arkitekt will act as a central server for your data, it needs to be
        accessible from within your network. We scanned this computer and tried
        to find all of the available network interfaces. Please select the
        interfaces that you want to use with Arkitekt. If you don't know what
        this means, just leave all of them selected.
      </div>
      <div className="mt-5  border  border-red-300 p-3 rounded rounded-md text-">
        For security reasons arkitekt will not allow any requests coming from
        devices that are trying to connect to arkitekt through other ip adresses
        or hostnames
      </div>
      <div className="mt-5 ">
        <HostSelectionField name="bindings" />
      </div>
    </div>
  );
};
