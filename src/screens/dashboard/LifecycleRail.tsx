import { Check, CircleAlert } from "lucide-react";

import { cn } from "../../utils";
import { TONE } from "./tone";
import type { Stage } from "./lifecycle";

// --- the lifecycle rail -----------------------------------------------------

const STAGE_TONE: Record<Stage["state"], string> = {
  done: TONE.ok,
  attention: TONE.waiting,
  waiting: TONE.idle,
};

/**
 * The four stages, side by side, each showing whether it happened and the one fact that
 * says when. A stage nobody has reached yet is drawn muted rather than hidden: the point
 * of the rail is that the missing step is as visible as the finished ones.
 */
export const LifecycleRail = ({ stages: list }: { stages: Stage[] }) => (
  <div className="grid gap-2 grid-cols-1 sm:grid-cols-2 lg:grid-cols-4">
    {list.map((stage, index) => (
      <div
        key={stage.key}
        className={cn(
          "rounded-lg border px-3 py-2.5 min-w-0",
          STAGE_TONE[stage.state]
        )}
      >
        <div className="flex items-center gap-2">
          <span className="flex size-5 shrink-0 items-center justify-center rounded-full border border-current text-[10px] font-semibold">
            {stage.state === "done" ? (
              <Check className="size-3" />
            ) : stage.state === "attention" ? (
              <CircleAlert className="size-3" />
            ) : (
              index + 1
            )}
          </span>
          <span className="text-sm font-medium">{stage.label}</span>
        </div>
        <div
          className="mt-1 text-xs text-muted-foreground truncate"
          title={stage.detail}
        >
          {stage.detail}
        </div>
      </div>
    ))}
  </div>
);
