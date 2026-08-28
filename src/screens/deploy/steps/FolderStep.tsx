import { FolderOpen, Loader2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useFormContext, useFormState, useWatch } from "react-hook-form";
import { Alert } from "../../../components/ui/alert";
import { Button } from "../../../components/ui/button";
import { Card } from "../../../components/ui/card";
import { ErrorDisplay } from "../../../components/Error";
import { UIField } from "../../../components/FormInput";
import { basename } from "../hub-form";
import * as api from "../../../api";
import { useRegistry } from "../../../registry/registry-context";
import { AdvancedFields, StepField, StepFrame } from "../../wizard/StepFrame";

/**
 * Where the deployment lives. Everything written — the config, the compose file, the
 * database directory — ends up in this folder, so it is shown rather than hidden inside
 * the app's data directory.
 *
 * The step opens on a folder that already works (`~/MyHub`, created on the way in) so
 * the common answer costs nothing, and "Change" is there for the rest.
 */
export const FolderStep = ({ kind }: { kind: { label: string } }) => {
  const { setValue } = useFormContext();
  const { dirtyFields } = useFormState({ name: "identifier" });
  const { pickFolder, suggestFolder, inspectFolder } = useRegistry();
  const path = useWatch({ name: "path" }) as string | undefined;
  const [verdict, setVerdict] = useState<api.FolderReport | undefined>();
  const [checking, setChecking] = useState(false);
  const [suggesting, setSuggesting] = useState(false);

  // Once per mount: coming back to the step must not move a folder the user chose,
  // and must not create a second `MyHub-2` behind their back.
  const suggested = useRef(false);

  /**
   * Names this folder suggests. The hub identifier comes along for the ride: it is the
   * next question but one, and the folder is already the name the user picked for this
   * deployment. An identifier they have typed themselves is never overwritten.
   */
  const adoptFolder = useCallback(
    async (picked: string) => {
      setValue("path", picked, { shouldValidate: true });
      setValue("name", basename(picked), { shouldValidate: true });
      if (!dirtyFields.identifier) {
        const suggested = await api.identifierFromFolder(picked).catch(() => "");
        setValue("identifier", suggested, { shouldValidate: true });
      }
    },
    [dirtyFields.identifier, setValue]
  );

  useEffect(() => {
    if (suggested.current || (path && path.length > 0)) return;
    suggested.current = true;
    setSuggesting(true);
    suggestFolder().then((picked) => {
      setSuggesting(false);
      if (picked) adoptFolder(picked);
    });
  }, [path, suggestFolder, adoptFolder]);

  useEffect(() => {
    let cancelled = false;
    if (!path) {
      setVerdict(undefined);
      return;
    }
    setChecking(true);
    inspectFolder(path).then((result) => {
      if (cancelled) return;
      setVerdict(result);
      setValue("folderOk", result.ok, { shouldValidate: true });
      setChecking(false);
    });
    return () => {
      cancelled = true;
    };
  }, [path, inspectFolder, setValue]);

  const choose = async () => {
    const picked = await pickFolder(`Choose a folder for this ${kind.label}`);
    if (!picked) return;
    adoptFolder(picked);
  };

  const message = verdict?.message ?? "";

  return (
    <StepFrame
      icon={FolderOpen}
      title="Where should it live?"
      subtitle={`A folder for your ${kind.label.toLowerCase()}`}
      lead={
        <>
          The configuration, the generated <code>docker-compose.yaml</code> and the
          database files all live in this folder. Keep it somewhere you can find again —
          you can manage the deployment from a terminal there too.
        </>
      }
    >
      <div className="max-w-xl flex flex-col gap-5">
        <Card className="gap-0 py-3 px-4 border-border">
          <div className="flex items-center gap-3">
            <span className="flex size-8 shrink-0 items-center justify-center rounded-md border border-border text-muted-foreground">
              {suggesting ? (
                <Loader2 className="size-3.5 animate-spin" />
              ) : (
                <FolderOpen className="size-3.5" />
              )}
            </span>
            <div className="min-w-0 flex-1">
              <div className="text-xs text-muted-foreground">
                {suggesting ? "Preparing a folder…" : "This deployment's folder"}
              </div>
              <div className="text-sm truncate" title={path}>
                {path || "—"}
              </div>
            </div>
            <Button type="button" variant="outline" size="sm" onClick={choose}>
              Change
            </Button>
          </div>
        </Card>

        {checking && (
          <div className="text-sm text-muted-foreground">Checking the folder…</div>
        )}

        {message && verdict && (
          <Alert variant={verdict.ok ? "default" : "destructive"}>{message}</Alert>
        )}

        <AdvancedFields fields={["name"]}>
          <StepField
            label="Name"
            hint="How this deployment is labelled inside Konstruktor. Taken from the folder."
          >
            <UIField name="name" autoComplete="off" spellCheck="false" />
            <ErrorDisplay name="name" className="mt-1" />
          </StepField>
        </AdvancedFields>

        <ErrorDisplay name="path" />
        <ErrorDisplay name="folderOk" />
      </div>
    </StepFrame>
  );
};
