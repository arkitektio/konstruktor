import {
  AlertTriangle,
  Check,
  FileCode2,
  History,
  Loader2,
  RotateCcw,
  Save,
  ShieldCheck,
} from "lucide-react";
import React, { useCallback, useEffect, useState } from "react";
import { useParams } from "react-router-dom";

import { AppMenu } from "../components/AppMenu";
import { Alert } from "../components/ui/alert";
import { Button } from "../components/ui/button";
import * as api from "../api";
import { Page } from "../layout/Page";
import { PageHeader } from "../layout/PageHeader";
import type { DeploymentRecord } from "../api";
import { useRegistry } from "../registry/registry-context";
import { useAlerter } from "../alerter/alerter-context";
import { cn } from "../utils";

/**
 * `docker-compose.yaml`, editable in place.
 *
 * The generator writes the file once and never touches it again, so it is the one lever
 * a person has over how the stack runs that Konstruktor does not mediate: an extra
 * volume, a resource limit, a port, an image pinned to something else. This is a plain
 * text area on purpose — a form over the compose specification would be a bigger project
 * than the app — with the three things a text area alone would not give:
 *
 * * a save that keeps the previous file, so one bad edit is one step back;
 * * Docker's own verdict on what was saved, since the core only checks it is YAML;
 * * what the generator would write from the profile today, to reset to.
 *
 * Nothing here restarts anything. The dashboard's Recreate is what applies a change,
 * and it says so at the bottom.
 */
const Editor: React.FC<{ deployment: DeploymentRecord }> = ({ deployment }) => {
  const { alert } = useAlerter();
  const [onDisk, setOnDisk] = useState<string>("");
  const [draft, setDraft] = useState<string>("");
  const [generated, setGenerated] = useState<string>("");
  const [hasBackup, setHasBackup] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** Docker's complaint about the saved file, or `""` once it has accepted it. */
  const [verdict, setVerdict] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);

  const dirty = draft !== onDisk;

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const view = await api.readComposeFile(deployment.path);
      setOnDisk(view.contents);
      setDraft(view.contents);
      setGenerated(view.generated);
      setHasBackup(view.has_backup);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setLoading(false);
    }
  }, [deployment.path]);

  useEffect(() => {
    void load();
  }, [load]);

  const check = useCallback(async () => {
    setChecking(true);
    try {
      const problem = await api.validateComposeFile(deployment.path);
      setVerdict(problem ?? "");
    } catch (e) {
      // The engine could not be asked; that is not a verdict on the file.
      alert({
        error: "Could not validate the file",
        message: typeof e === "string" ? e : String(e),
        subtitle: "Docker did not answer.",
      });
    } finally {
      setChecking(false);
    }
  }, [deployment.path, alert]);

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await api.writeComposeFile(deployment.path, draft);
      setOnDisk(draft);
      setHasBackup(true);
      // Straight after a save, so a mistake is shown where it was made.
      await check();
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setSaving(false);
    }
  };

  const restorePrevious = async () => {
    try {
      const previous = await api.readComposeBackup(deployment.path);
      if (previous !== null) setDraft(previous);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    }
  };

  return (
    <Page
      menu={
        <AppMenu
          back={`/dashboard/${deployment.id}`}
          breadcrumb={`${deployment.name} · docker-compose.yaml`}
        />
      }
    >
      <div className="flex flex-col gap-4">
        <PageHeader
          icon={FileCode2}
          title="Compose file"
          subtitle={deployment.name}
          actions={
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                disabled={loading || checking || dirty}
                title={
                  dirty
                    ? "Save first — Docker reads the file on disk"
                    : "Ask Docker whether it accepts the file on disk"
                }
                onClick={() => void check()}
              >
                {checking ? (
                  <Loader2 className="size-3.5 animate-spin" />
                ) : (
                  <ShieldCheck className="size-3.5" />
                )}
                Validate
              </Button>
              <Button
                size="sm"
                disabled={loading || saving || !dirty}
                onClick={() => void save()}
              >
                {saving ? (
                  <Loader2 className="size-3.5 animate-spin" />
                ) : (
                  <Save className="size-3.5" />
                )}
                {saving ? "Saving…" : "Save"}
              </Button>
            </div>
          }
        />

        <p className="text-sm text-muted-foreground max-w-2xl leading-relaxed">
          This is the file Docker Compose runs. Konstruktor generated it when the hub was
          created and does not rewrite it, so anything you change here stays. The previous
          version is kept as <code>docker-compose.yaml.bak</code> on every save.
        </p>

        {error && (
          <Alert variant="destructive" className="max-w-2xl">
            {error}
          </Alert>
        )}

        {verdict !== null &&
          (verdict === "" ? (
            <Alert className="max-w-2xl" data-testid="compose-valid">
              <Check />
              Docker accepts this file.
            </Alert>
          ) : (
            <Alert variant="destructive" className="max-w-2xl" data-testid="compose-invalid">
              <AlertTriangle />
              <div className="flex flex-col gap-1">
                <span className="font-medium">
                  Saved, but Docker does not accept it — nothing will start until this is
                  fixed.
                </span>
                <pre className="text-xs whitespace-pre-wrap font-mono">{verdict}</pre>
              </div>
            </Alert>
          ))}

        <div className="rounded-lg border border-border bg-card overflow-hidden">
          <textarea
            aria-label="docker-compose.yaml"
            spellCheck={false}
            autoCorrect="off"
            autoCapitalize="off"
            value={draft}
            disabled={loading}
            onChange={(event) => setDraft(event.target.value)}
            className={cn(
              "w-full min-h-[60vh] resize-y bg-transparent p-3 font-mono text-xs leading-relaxed outline-none",
              loading && "opacity-50"
            )}
          />
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="ghost"
            size="sm"
            disabled={!dirty}
            onClick={() => setDraft(onDisk)}
          >
            <RotateCcw className="size-3.5" />
            Discard changes
          </Button>
          <Button
            variant="ghost"
            size="sm"
            disabled={!hasBackup}
            title="Load docker-compose.yaml.bak into the editor"
            onClick={() => void restorePrevious()}
          >
            <History className="size-3.5" />
            Load previous version
          </Button>
          {generated && (
            <Button
              variant="ghost"
              size="sm"
              disabled={draft === generated}
              title="Load what the generator would write from the profile today"
              onClick={() => setDraft(generated)}
            >
              <FileCode2 className="size-3.5" />
              Load generated file
            </Button>
          )}
          <span className="ml-auto text-xs text-muted-foreground">
            Changes apply on the next <em>Recreate containers</em> from the dashboard.
          </span>
        </div>
      </div>
    </Page>
  );
};

export const ComposeEditorScreen: React.FC = () => {
  const { id } = useParams<{ id: string }>();
  const { byId } = useRegistry();
  const deployment = id ? byId(id) : undefined;

  return deployment ? (
    <Editor deployment={deployment} />
  ) : (
    <>Could not find this deployment</>
  );
};
