import { cn } from "../utils";

/**
 * The heading a screen wears, matching the wizard's `StepFrame`.
 *
 * The two exist separately because a step sits inside a frame that already names what is
 * being created, while a screen has to introduce itself — but the type scale, the icon
 * chip and the spacing are the same on purpose. Walking out of the wizard and onto the
 * dashboard should not feel like walking into a different application.
 */
export const PageHeader = ({
  icon: Icon,
  title,
  subtitle,
  badge,
  actions,
  className,
}: {
  icon?: React.ComponentType<{ className?: string }>;
  title: React.ReactNode;
  subtitle?: React.ReactNode;
  /** Sits next to the title — a kind, a status. */
  badge?: React.ReactNode;
  /** Sits opposite the title, pushed to the end of the row. */
  actions?: React.ReactNode;
  className?: string;
}) => (
  <div className={cn("flex items-start gap-3", className)}>
    {Icon && (
      <span className="mt-0.5 flex size-9 shrink-0 items-center justify-center rounded-lg border border-border bg-card text-primary">
        <Icon className="size-4.5" />
      </span>
    )}
    <div className="min-w-0 flex-1">
      <div className="flex items-center gap-2 min-w-0">
        <h1 className="text-2xl font-bold tracking-tight truncate">{title}</h1>
        {badge}
      </div>
      {subtitle && (
        <div className="text-sm text-muted-foreground mt-0.5">{subtitle}</div>
      )}
    </div>
    {actions && <div className="shrink-0 flex items-center gap-2">{actions}</div>}
  </div>
);

/** The heading of a block within a screen — "Services", "Danger zone". */
export const SectionHeading = ({
  children,
  hint,
  className,
}: {
  children: React.ReactNode;
  hint?: React.ReactNode;
  className?: string;
}) => (
  <div className={cn("mb-2", className)}>
    <h2 className="text-sm font-medium uppercase tracking-wider text-muted-foreground">
      {children}
    </h2>
    {hint && <div className="text-xs text-muted-foreground mt-0.5">{hint}</div>}
  </div>
);
