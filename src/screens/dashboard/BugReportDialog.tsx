import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open as openExternal } from "@tauri-apps/plugin-shell";
import { Bug, Check, ClipboardCopy, ExternalLink, Loader2, ShieldCheck } from "lucide-react";
import { useEffect, useState } from "react";

import * as api from "../../api";
import type { BugReport } from "../../api";
import { Alert } from "../../components/ui/alert";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../../components/ui/dialog";
import { ScrollArea } from "../../components/ui/scroll-area";

/**
 * Filing a bug against the service, with its log attached.
 *
 * The report is built in Rust — see `report.rs` — because that is where the deployment's
 * configuration is, and the log is matched against this hub's *actual* credentials rather
 * than against a guess at what a secret looks like.
 *
 * The whole report is shown before anything happens. That is not politeness: the log is
 * about to be pasted into a public issue tracker, and the only person who can tell
 * whether the redaction caught everything is the one who knows what is in it. Nothing is
 * sent from here — the clipboard and a browser tab are the only things this touches, so
 * the user can still walk away from the GitHub page having published nothing.
 */
export const BugReportDialog = ({
  open,
  onOpenChange,
  path,
  service,
  name,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The deployment folder — the log and the secrets both come from it. */
  path: string;
  /** The compose service, which is how the profile and the containers are matched. */
  service: string;
  /** What to call it in the heading. */
  name: string;
}) => {
  const [report, setReport] = useState<BugReport | undefined>();
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  // Rebuilt every time it opens: the log is the point, and yesterday's is worthless.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setReport(undefined);
    setError(null);
    setCopied(false);
    api
      .bugReport(path, service)
      .then((built) => !cancelled && setReport(built))
      .catch((e) => !cancelled && setError(typeof e === "string" ? e : String(e)));
    return () => {
      cancelled = true;
    };
  }, [open, path, service]);

  const copy = async () => {
    if (!report) return;
    await writeText(report.body);
    setCopied(true);
  };

  /**
   * Copy first, then open. The issue page carries the title and the environment; the log
   * is far too long for a query string — GitHub refuses one — so it is pasted in, and
   * copying before the tab opens is what makes that one keystroke rather than a hunt back
   * through this app.
   */
  const copyAndOpen = async () => {
    if (!report?.issueUrl) return;
    await copy();
    await openExternal(report.issueUrl);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Bug className="size-4" />
            Report a bug in {name}
          </DialogTitle>
          <DialogDescription>
            {report?.repo
              ? `This goes to ${report.repo.replace("https://github.com/", "")}, where the service's code lives — not to Konstruktor.`
              : "Everything below is prepared for you; nothing has been sent."}
          </DialogDescription>
        </DialogHeader>

        {error && <Alert variant="destructive">{error}</Alert>}

        {report && !report.repo && (
          <Alert>
            The profile does not say where this service's code lives, so there is no
            repository to file against. The report is still here to copy.
          </Alert>
        )}

        {report?.logError && (
          <Alert>
            The log could not be read: {report.logError}. The report says so rather than
            pretending the service printed nothing.
          </Alert>
        )}

        {report && (
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <ShieldCheck className="size-3.5 text-success shrink-0" />
            {report.redactions > 0 ? (
              <span>
                <Badge variant="outline" className="mr-1.5 font-normal">
                  {report.redactions} removed
                </Badge>
                values this deployment's own configuration recognises as credentials were
                replaced. Read the log below before you publish it.
              </span>
            ) : (
              <span>
                Nothing in this log matched a credential from this deployment. Read it
                before you publish it anyway — you know what is in it.
              </span>
            )}
          </div>
        )}

        <ScrollArea className="h-[45vh] w-full rounded-md border border-border bg-background p-3">
          {report ? (
            <pre className="text-[11px] leading-relaxed font-mono whitespace-pre-wrap break-all">
              {report.body}
            </pre>
          ) : (
            <span className="text-sm text-muted-foreground inline-flex items-center gap-2">
              <Loader2 className="size-3.5 animate-spin" />
              Reading the log and taking the secrets out…
            </span>
          )}
        </ScrollArea>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Close
          </Button>
          <Button variant="outline" disabled={!report} onClick={() => void copy()}>
            {copied ? <Check className="size-3.5" /> : <ClipboardCopy className="size-3.5" />}
            {copied ? "Copied" : "Copy the report"}
          </Button>
          <Button disabled={!report?.issueUrl} onClick={() => void copyAndOpen()}>
            <ExternalLink className="size-3.5" />
            Copy and open GitHub
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
