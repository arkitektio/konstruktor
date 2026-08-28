import type { Container } from "../../api";
import { isInitContainer, type RunState } from "./lifecycle";

/**
 * The colour vocabulary every piece of the dashboard shares.
 *
 * It has a file of its own because each card reaches for it, and because one place to
 * look is what stops the next card from reaching for a raw palette colour instead.
 */

/**
 * Status reads from `--success` / `--warning` / `--destructive`, never from a raw
 * `green-500`.
 *
 * The brand hue is a knob the user turns (see `lib/brand.ts`), so anything hard-coded to
 * a Tailwind palette colour drifts out of the theme the moment they do. Status colours
 * stay categorical — green is fine, amber is waiting, red is wrong, whatever the brand
 * is set to — but they are categorical *tokens*, tuned once per theme in `globals.css`.
 */
export const TONE = {
  ok: "border-success/50 bg-success/10 text-success",
  waiting: "border-warning/60 bg-warning/10 text-warning",
  bad: "border-destructive/50 bg-destructive/10 text-destructive",
  idle: "border-border bg-muted text-muted-foreground",
} as const;

export const containerColor = (container: Container) => {
  // An init container that has exited did what it was started to do. Red there is a
  // false alarm, and the one on this page that never goes away.
  if (isInitContainer(container)) return "border-muted-foreground/30";
  if (container.state === "running") return "border-success/50";
  if (container.state === "exited") return "border-destructive/50";
  return "border-muted-foreground/30";
};

export const serviceColor = (containers: Container[]) => {
  if (containers.length === 0) return "bg-muted border-muted-foreground/30";
  if (containers.every((c) => c.state === "running"))
    return "bg-success/10 border-success/50";
  if (containers.some((c) => c.state === "running"))
    return "bg-warning/10 border-warning/60";
  return "bg-destructive/10 border-destructive/50";
};

/**
 * "Something is waiting for you" is not "something is broken".
 *
 * Amber, never `destructive`: a hub with a pulled-but-unapplied image is working fine,
 * and the red badge that says otherwise is the one thing a user sees first.
 */
export const PENDING_BADGE = TONE.waiting;

export const RUN_STATE_DOT: Record<RunState, string> = {
  running: "bg-success",
  partial: "bg-warning",
  stopped: "bg-destructive",
  never: "bg-muted-foreground/40",
};
