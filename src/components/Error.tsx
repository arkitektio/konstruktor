import { get, useFormContext, useFormState } from "react-hook-form";
import { Alert } from "./ui/alert";

export type ErrorDisplayProps = {
  name: string;
  className?: string;
  children?: (message: string) => React.ReactNode;
};

/**
 * Renders the validation error of a single field, if it has one.
 */
export const ErrorDisplay = ({ name, className, children }: ErrorDisplayProps) => {
  const { control } = useFormContext();
  // useFormState subscribes this component to error updates, a plain
  // `formState.errors` read through the context would not re-render it. The
  // subscription is deliberately not scoped to `name`: array level errors are
  // reported under `<array>.root`, which a name scoped subscription misses.
  const { errors } = useFormState({ control });

  const error = get(errors, name);
  const message = error?.message;

  if (typeof message !== "string" || message.length === 0) {
    return null;
  }

  if (children) {
    return <div className={className}>{children(message)}</div>;
  }

  return (
    <Alert variant="destructive" className={className}>
      {message}
    </Alert>
  );
};
