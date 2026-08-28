import { useFormContext } from "react-hook-form";
import { cn } from "../utils";

export type UIFieldProps = React.ComponentProps<"input"> & {
  name: string;
};

export const UIField = ({ name, className, ...props }: UIFieldProps) => {
  const { register } = useFormContext();

  return (
    <input
      {...register(name)}
      {...props}
      className={cn(
        "flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
        className
      )}
    />
  );
};
