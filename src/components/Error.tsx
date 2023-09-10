import { ErrorMessage, ErrorMessageProps } from "formik";
import { Alert } from "./ui/alert";

export const ErrorDisplay = (props: ErrorMessageProps) => {
  return (
    <ErrorMessage {...props}>
      {(message) => <Alert variant="destructive">{message}</Alert>}
    </ErrorMessage>
  );
};
