import type { ComposeLine } from "./api";

/**
 * Compose's own output, kept for reading rather than boiled down.
 *
 * With `--progress plain` a pull reports every layer, again and again —
 * ` 034d6572bf28 Downloading 12.5MB` — so a layer gets one line that each report
 * replaces, and the log reads as the state of every download rather than a scroll of
 * them. Images and containers fold the same way, `Pulling` becoming `Pulled` in place.
 * Everything else is appended as it comes.
 */
export type LogEntry = {
  key: string;
  text: string;
  stderr: boolean;
  /** A line that says what went wrong, to be drawn as such. */
  error: boolean;
};

/** ` Image caddy:2.11.4 Pulling`, ` Container hub-db-1  Started` — one line per subject. */
const SUBJECT = /^\s*(Container|Image|Network|Volume)\s+(\S+)\s+.+$/;
/** A layer's digest prefix, then what is happening to it. */
const LAYER = /^\s*([0-9a-f]{12})\s+(.+?)\s*$/;
const ERROR = /\berror\b|\bdenied\b|\bnot found\b|\bfailed\b/i;

/** Enough for any pull; a runaway stream stops growing the page past it. */
const MAX_ENTRIES = 2000;

export const appendLine = (entries: LogEntry[], { line, stderr }: ComposeLine): LogEntry[] => {
  const text = line.trimEnd();
  if (!text.trim()) return entries;
  const error = ERROR.test(text);

  const subject = SUBJECT.exec(text);
  const layer = subject ? null : LAYER.exec(text);
  const key = subject ? `${subject[1]}:${subject[2]}` : layer ? `layer:${layer[1]}` : null;
  if (key) {
    const at = entries.findIndex((e) => e.key === key);
    const entry = { key, text: text.trim(), stderr, error };
    if (at >= 0) {
      const next = entries.slice();
      next[at] = entry;
      return next;
    }
    return [...entries, entry].slice(-MAX_ENTRIES);
  }
  return [...entries, { key: `line:${entries.length}:${text}`, text, stderr, error }].slice(
    -MAX_ENTRIES
  );
};

/**
 * The lines worth leading a failure with: compose's own `Error …` lines, deduplicated —
 * it reports a refused pull once for the image and once more from the daemon.
 */
export const errorLines = (output: string): string[] => {
  const seen = new Set<string>();
  const found: string[] = [];
  for (const raw of output.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || !ERROR.test(line)) continue;
    const key = line.replace(/^Image \S+ Error\s+/, "").replace(/^Error response from daemon:\s*/, "");
    if (seen.has(key)) continue;
    seen.add(key);
    found.push(line);
  }
  return found;
};
