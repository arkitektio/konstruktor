import { Separator } from "../components/ui/separator";
import { cn } from "../utils";

/**
 * The heading a screen wears.
 *
 * Deliberately the same shape as Kontrol's `components/PageHeader.tsx`: a square muted
 * tile holding the icon, a bold title, a muted description, actions pushed to the end,
 * and a separator underneath that starts the page's rhythm. Konstruktor hands a hub over
 * to Kontrol once it is authorized, and the two headers being the same object is most of
 * what makes that feel like one application rather than two.
 *
 * What is kept from the old header, because Kontrol's pages have nowhere to put them:
 * the `badge` slot next to the title (a kind, a run state) and the optional icon — a
 * screen that is only reporting an error has no icon worth showing.
 */
export const PageHeader = ({
  icon: Icon,
  title,
  subtitle,
  badge,
  actions,
  className,
  separator = true,
}: {
  icon?: React.ComponentType<{ className?: string }>;
  title: React.ReactNode;
  subtitle?: React.ReactNode;
  /** Sits next to the title — a kind, a status. */
  badge?: React.ReactNode;
  /** Sits opposite the title, pushed to the end of the row. */
  actions?: React.ReactNode;
  className?: string;
  /** The rule underneath. Off for a header that is already inside a bordered card. */
  separator?: boolean;
}) => (
  <div className={cn("flex flex-col gap-4", className)}>
    <div className="flex items-start justify-between gap-4">
      <div className="flex items-center gap-4 min-w-0">
        {Icon && (
          <span className="flex size-14 shrink-0 items-center justify-center rounded-lg bg-muted text-foreground">
            <Icon className="size-7" />
          </span>
        )}
        <div className="min-w-0 space-y-1">
          <div className="flex items-center gap-2 min-w-0 flex-wrap">
            <h1 className="text-2xl font-bold tracking-tight truncate">{title}</h1>
            {badge}
          </div>
          {subtitle && (
            <div className="text-sm text-muted-foreground">{subtitle}</div>
          )}
        </div>
      </div>
      {actions && (
        <div className="shrink-0 flex items-center gap-2">{actions}</div>
      )}
    </div>
    {separator && <Separator />}
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
  <div className={cn("mb-3", className)}>
    <h2 className="text-sm font-medium uppercase tracking-wider text-muted-foreground">
      {children}
    </h2>
    {hint && <div className="text-xs text-muted-foreground mt-0.5">{hint}</div>}
  </div>
);
