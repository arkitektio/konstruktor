import { Check, CircleAlert } from "lucide-react";

import { cn } from "../../utils";
import type { Stage } from "./lifecycle";

// --- the lifecycle rail -----------------------------------------------------

const STAGE_TONE: Record<Stage["state"], string> = {
  done: "text-success border-success/40",
  attention: "text-warning border-warning/60",
  waiting: "text-muted-foreground border-border",
};

/**
 * The four stages in one line, each a chip with a tick, a warning, or its number. The
 * fact that says *when* lives in the tooltip: the rail's job is to make the missing step
 * as visible as the finished ones, which one row of chips does without a row of cards.
 */
export const LifecycleRail = ({ stages: list }: { stages: Stage[] }) => (
  <div className="flex flex-wrap items-center gap-1.5">
    {list.map((stage, index) => (
      <div
        key={stage.key}
        title={stage.detail}
        className={cn(
          "flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs",
          STAGE_TONE[stage.state]
        )}
      >
        <span className="flex size-4 shrink-0 items-center justify-center rounded-full border border-current text-[9px] font-semibold">
          {stage.state === "done" ? (
            <Check className="size-2.5" />
          ) : stage.state === "attention" ? (
            <CircleAlert className="size-2.5" />
          ) : (
            index + 1
          )}
        </span>
        <span className="font-medium text-foreground/90">{stage.label}</span>
        {stage.state !== "done" && (
          <span className="text-muted-foreground max-w-[28ch] truncate hidden sm:inline">
            {stage.detail}
          </span>
        )}
      </div>
    ))}
  </div>
);
