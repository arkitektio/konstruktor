import type { ComposeLine } from "./api";

/**
 * What compose has said so far, boiled down to a fraction and a phrase.
 *
 * Compose narrates each container on its own line — ` Container hub-db-1  Starting`, then
 * ` Container hub-db-1  Started` — and images the same way on a pull. The first time a
 * name appears it is a thing to do; a past-tense verb is that thing done. The fraction is
 * the one against the other, and the phrase is the last line, shortened.
 */
export type ComposeProgress = {
  /** 0..1, or `undefined` before compose has named anything. */
  fraction: number | undefined;
  /** The last thing compose said, e.g. "db · Starting". */
  step: string | undefined;
  done: number;
  total: number;
};

export const EMPTY_PROGRESS: ComposeProgress = {
  fraction: undefined,
  step: undefined,
  done: 0,
  total: 0,
};

const SUBJECT = /^\s*(Container|Image|Network|Volume)\s+(\S+)\s+(.+?)\s*$/;
/** Past tense, or a final state: the line that closes a subject. */
const FINISHED = /^(Started|Created|Running|Healthy|Stopped|Removed|Pulled|Recreated|Killed|Exited|Waiting|Error)\b/;

export type ProgressState = { seen: Set<string>; done: Set<string> };

export const newProgressState = (): ProgressState => ({ seen: new Set(), done: new Set() });

export const advance = (
  state: ProgressState,
  { line }: ComposeLine,
  /** The compose project name, so `hub-db-1` reads as `db`. */
  project?: string
): ComposeProgress => {
  const match = SUBJECT.exec(line);
  if (!match) {
    return summarize(state, undefined);
  }
  const [, kind, name, verb] = match;
  const key = `${kind}:${name}`;
  state.seen.add(key);
  if (FINISHED.test(verb)) state.done.add(key);
  return summarize(state, `${shortName(name, project)} · ${verb}`);
};

const summarize = (state: ProgressState, step: string | undefined): ComposeProgress => {
  const total = state.seen.size;
  const done = state.done.size;
  return {
    fraction: total === 0 ? undefined : done / total,
    step,
    done,
    total,
  };
};

/** `hub-db-1` → `db` when the project is `hub`; images lose their registry path. */
const shortName = (name: string, project?: string): string => {
  let short = name;
  if (project && short.startsWith(`${project}-`)) short = short.slice(project.length + 1);
  short = short.replace(/-\d+$/, "");
  if (short.includes("/")) short = short.split("/").pop() ?? short;
  return short;
};
