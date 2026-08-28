import { ChevronDown } from "lucide-react";
import { useState } from "react";
import { get, useFormState } from "react-hook-form";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "../../components/ui/collapsible";
import { cn } from "../../utils";

/**
 * The heading every wizard step wears.
 *
 * Steps used to each spell out their own `text-6xl font-light` hero, which drifted step
 * by step. The type scale here is the one orkestrator-next uses for page headers, so a
 * step of the installer and a page of the app read as the same product.
 */
export const StepFrame = ({
  icon: Icon,
  title,
  subtitle,
  lead,
  children,
  className,
}: {
  icon?: React.ComponentType<{ className?: string }>;
  title: string;
  subtitle?: string;
  /** The paragraph that explains why the step is being asked at all. */
  lead?: React.ReactNode;
  children?: React.ReactNode;
  className?: string;
}) => (
  <div className={cn("flex flex-col max-w-3xl", className)}>
    <div className="flex items-start gap-3">
      {Icon && (
        <span className="mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-lg border border-border bg-card text-primary">
          <Icon className="size-4.5" />
        </span>
      )}
      <div className="min-w-0">
        <h1 className="text-2xl font-bold tracking-tight">{title}</h1>
        {subtitle && (
          <p className="text-sm text-muted-foreground mt-0.5">{subtitle}</p>
        )}
      </div>
    </div>

    {lead && (
      <p className="text-sm text-muted-foreground leading-relaxed mt-4 max-w-xl">
        {lead}
      </p>
    )}

    {children && <div className="mt-6">{children}</div>}
  </div>
);

/** A labelled block around a single field, with its hint underneath. */
export const StepField = ({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: React.ReactNode;
  children: React.ReactNode;
}) => (
  <div className="flex flex-col gap-1.5">
    <div className="text-sm font-medium">{label}</div>
    {children}
    {hint && <div className="text-xs text-muted-foreground">{hint}</div>}
  </div>
);

/**
 * The fields a step does not need answered, folded away by default.
 *
 * Every one of these has a working default, so showing them next to the question that
 * actually has to be answered buries it. The disclosure opens itself when one of the
 * fields it hides has a validation error — a "Next" that is disabled for a reason the
 * user cannot see is worse than a step with too many fields on it.
 */
export const AdvancedFields = ({
  label = "Advanced",
  /** The form field names inside, so an error in one can force this open. */
  fields = [],
  children,
}: {
  label?: string;
  fields?: string[];
  children: React.ReactNode;
}) => {
  const { errors } = useFormState();
  const [open, setOpen] = useState(false);

  const broken = fields.some((name) => get(errors, name)?.message);

  return (
    <Collapsible open={open || broken} onOpenChange={setOpen}>
      <CollapsibleTrigger asChild>
        <button
          type="button"
          className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
        >
          <ChevronDown
            className={cn(
              "size-3.5 transition-transform",
              (open || broken) && "rotate-180"
            )}
          />
          {label}
        </button>
      </CollapsibleTrigger>
      <CollapsibleContent className="pt-4 flex flex-col gap-5">
        {children}
      </CollapsibleContent>
    </Collapsible>
  );
};
