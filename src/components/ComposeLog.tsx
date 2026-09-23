import { useEffect, useRef } from "react";

import type { LogEntry } from "../compose-log";
import { cn } from "../utils";

/**
 * Compose's output as it streams, one line per image, container and layer. Follows the
 * end while the user is at the end, and stays put once they scroll up to read.
 */
export const ComposeLog = ({
  entries,
  className,
  empty = "Waiting for docker compose…",
}: {
  entries: LogEntry[];
  className?: string;
  empty?: string;
}) => {
  const box = useRef<HTMLDivElement>(null);
  const following = useRef(true);

  useEffect(() => {
    const el = box.current;
    if (el && following.current) el.scrollTop = el.scrollHeight;
  }, [entries]);

  return (
    <div
      ref={box}
      onScroll={(e) => {
        const el = e.currentTarget;
        following.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
      }}
      className={cn(
        "max-h-72 overflow-auto rounded-md border border-border bg-muted/40 px-3 py-2",
        "font-mono text-xs leading-relaxed whitespace-pre-wrap break-all",
        className
      )}
    >
      {entries.length === 0 ? (
        <div className="text-muted-foreground">{empty}</div>
      ) : (
        entries.map((entry) => (
          <div
            key={entry.key}
            className={
              entry.error
                ? "text-destructive"
                : entry.key.startsWith("layer:")
                  ? "pl-4 text-muted-foreground"
                  : "text-foreground/80"
            }
          >
            {entry.text}
          </div>
        ))
      )}
    </div>
  );
};
