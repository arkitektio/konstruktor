import { DialogTitle } from "@radix-ui/react-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
} from "../components/ui/dialog";
import { useRepo } from "./repo-context";
import { useEffect } from "react";
import React from "react";
import { useAlerter } from "../alerter/alerter-context";



export const RepoSearcher = (props: { url: string }) => {
  const { validate } = useRepo();
  const { alert, catchAlert } = useAlerter();

  const [open, setOpen] = React.useState(false);

  useEffect(() => {
    fetch(props.url)
      .then((response) => {
        if (response.status !== 200) {
          alert({
            message: "Error fetching channels",
            error: response.statusText,
            subtitle: "Please try again later",
          });
          return;
        }
        response.json().then((data) => {
          validate(data);
        });
      })
      .catch(catchAlert);
  }, [props.url]);

  return <></>;
};
