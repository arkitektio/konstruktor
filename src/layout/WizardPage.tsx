import { motion } from "motion/react";
import { Check } from "lucide-react";
import { useEffect, useRef } from "react";
import { ScrollArea } from "../components/ui/scroll-area";
import { Logo } from "./Logo";
import { cn } from "../utils";
import type { WizardRailStep } from "../screens/wizard/Wizard";

/**
 * The frame a wizard runs inside.
 *
 * Deliberately not `Page`: the wizard wants a rail down the side, a progress bar and the
 * hero background the rest of Arkitekt uses on its connect screens, while Home, the
 * dashboard and the log viewer all depend on `Page`'s plain three-band layout.
 */
export const WizardPage = ({
  title,
  rail,
  position,
  total,
  onJump,
  buttons,
  children,
  stepKey,
}: {
  /** What is being created, shown next to the logo. */
  title: string;
  rail: WizardRailStep[];
  position: number;
  total: number;
  onJump: (index: number) => void;
  buttons?: React.ReactNode;
  children: React.ReactNode;
  /** Changes when the step changes, which is what drives the transition. */
  stepKey: string | number;
}) => {
  const progress = total > 0 ? (position / total) * 100 : 0;

  // The viewport survives the step's remount, so without this a short step opens
  // already scrolled past its own content.
  const viewport = useRef<HTMLDivElement>(null);
  useEffect(() => {
    viewport.current?.scrollTo({ top: 0 });
  }, [stepKey]);

  return (
    <div className="@container h-screen w-screen flex flex-col overflow-hidden bg-radial-[at_100%_100%] from-background to-backgroundpaired text-foreground">
      {/* Header */}
      <div className="shrink-0 flex items-center gap-3 px-6 py-4">
        <Logo
          width={28}
          height={28}
          cubeColor="var(--primary)"
          aColor="currentColor"
          strokeColor="currentColor"
        />
        <div className="flex-1 min-w-0">
          <div className="font-semibold tracking-tight leading-none">{title}</div>
          <div className="text-xs text-muted-foreground mt-0.5">
            Step {position} of {total}
          </div>
        </div>
      </div>

      {/* Progress bar */}
      <div className="shrink-0 h-0.5 bg-border/60">
        <motion.div
          className="h-full bg-primary"
          initial={false}
          animate={{ width: `${progress}%` }}
          transition={{ type: "spring", stiffness: 220, damping: 30 }}
        />
      </div>

      <div className="flex-1 min-h-0 flex flex-row">
        <WizardRail rail={rail} onJump={onJump} />

        <ScrollArea viewportRef={viewport} className="flex-1 min-w-0">
          <motion.div
            key={stepKey}
            initial={{ opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.22, ease: "easeOut" }}
            className="px-6 py-6 @container"
          >
            {children}
          </motion.div>
        </ScrollArea>
      </div>

      {/* Footer */}
      <div className="shrink-0 flex flex-row-reverse items-center gap-2 px-6 py-3 border-t border-border bg-card/60 backdrop-blur">
        {buttons}
      </div>
    </div>
  );
};

/**
 * The list of steps down the left. Steps already answered are clickable — going back to
 * change an answer is the one navigation a wizard should never make hard.
 */
const WizardRail = ({
  rail,
  onJump,
}: {
  rail: WizardRailStep[];
  onJump: (index: number) => void;
}) => (
  <nav className="hidden @2xl:flex shrink-0 w-52 flex-col gap-0.5 border-r border-border/60 px-3 py-6 overflow-y-auto">
    {rail.map((step) => {
      const Icon = step.meta?.icon;
      const done = step.status === "done";
      const current = step.status === "current";

      return (
        <button
          key={step.index}
          type="button"
          disabled={!done}
          onClick={() => onJump(step.index)}
          className={cn(
            "group flex items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-sm transition-colors",
            current && "bg-accent text-accent-foreground font-medium",
            done && "text-muted-foreground hover:bg-accent/50 cursor-pointer",
            !done && !current && "text-muted-foreground/50 cursor-default"
          )}
        >
          <span
            className={cn(
              "flex size-5 shrink-0 items-center justify-center rounded-full border text-[10px]",
              current && "border-primary text-primary",
              done && "border-primary bg-primary text-primary-foreground",
              !done && !current && "border-border"
            )}
          >
            {done ? (
              <Check className="size-3" />
            ) : Icon ? (
              <Icon className="size-3" />
            ) : null}
          </span>
          <span className="truncate">{step.meta?.label ?? `Step ${step.index + 1}`}</span>
        </button>
      );
    })}
  </nav>
);
