import React, { useEffect } from "react";
import { useAlerter } from "../alerter/alerter-context";
import channelsJSON from "./channels.json";
import { useRepo } from "./repo-context";


export const TestRepo = () => {
  const { validate } = useRepo();
  const { alert, catchAlert } = useAlerter();

  useEffect(() => {
    try {
    console.log("validating", channelsJSON)
    validate(channelsJSON)
    } catch (e) {
      console.error(e)
    }



  }, []);

  return <></>;
};
