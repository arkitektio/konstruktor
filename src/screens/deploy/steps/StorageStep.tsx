import { AlertTriangle, Database, FolderOpen, HardDrive } from "lucide-react";
import { useFormContext, useWatch } from "react-hook-form";
import { Alert } from "../../../components/ui/alert";
import { Card } from "../../../components/ui/card";
import { cn } from "../../../utils";
import { StepFrame } from "../../wizard/StepFrame";
import type { StorageMode } from "../../../api";
import { HubForm } from "../hub-form";

/**
 * Where the database and the object storage keep their bytes.
 *
 * The default is not up for much debate, which is why the step is short: a named Docker
 * volume lives inside the engine's own VM on macOS and Windows and on the host's disk on
 * Linux, and in every case that is the fastest storage a container can get. A bind mount
 * into the deployment folder goes through the desktop engines' file-sharing layer
 * instead — gRPC-FUSE, virtiofs — and Postgres over that is easily ten times slower on
 * writes. The folder mode exists for one reason, being able to *see* the data as a
 * directory, and the step says what it costs before anyone picks it.
 */

const OPTIONS: {
  value: StorageMode;
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  tag?: string;
  body: string;
}[] = [
  {
    value: "docker-volumes",
    icon: HardDrive,
    title: "Docker volumes",
    tag: "recommended",
    body: "Named volumes managed by Docker, inside its own virtual machine on macOS and Windows. The fastest option by a wide margin, and what everything here is tested against. The data is not a folder you can browse — the Back up action copies it out for you.",
  },
  {
    value: "deployment-folder",
    icon: FolderOpen,
    title: "Folders inside the deployment",
    body: "Bind-mount db_data/ and minio_data/ inside the deployment folder, so the data is a directory you can see, copy and move with the rest of the hub. Every read and write crosses the file-sharing layer on Docker Desktop.",
  },
];

export const StorageStep = () => {
  const { setValue } = useFormContext();
  const values = useWatch() as HubForm;
  const mode = values.storage ?? "docker-volumes";

  return (
    <StepFrame
      icon={Database}
      title="Storage"
      subtitle="Where the database and object storage keep their data"
      lead="Everything a hub stores — every row and every uploaded image — goes through these two mounts. Leave the default unless you have a specific reason to want the data as a folder."
    >
      <div className="max-w-xl flex flex-col gap-2">
        {OPTIONS.map((option) => {
          const selected = mode === option.value;
          const Icon = option.icon;
          return (
            <Card
              key={option.value}
              role="radio"
              aria-checked={selected}
              onClick={() =>
                setValue("storage", option.value, { shouldValidate: true })
              }
              className={cn(
                "gap-0 py-3 px-4 cursor-pointer border transition-colors",
                selected ? "border-primary bg-primary/5" : "border-border"
              )}
            >
              <div className="flex items-start gap-3">
                <span
                  className={cn(
                    "mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md border",
                    selected
                      ? "border-primary text-primary"
                      : "border-border text-muted-foreground"
                  )}
                >
                  <Icon className="size-3.5" />
                </span>
                <div className="min-w-0">
                  <div className="text-sm font-medium flex items-center gap-2">
                    {option.title}
                    {option.tag && (
                      <span className="rounded-full border border-primary/40 px-2 py-0.5 text-[10px] uppercase tracking-wide text-primary">
                        {option.tag}
                      </span>
                    )}
                  </div>
                  <p className="text-xs text-muted-foreground leading-relaxed mt-1">
                    {option.body}
                  </p>
                </div>
              </div>
            </Card>
          );
        })}

        {mode === "deployment-folder" && (
          <Alert variant="destructive" className="mt-2" data-testid="storage-warning">
            <AlertTriangle />
            <div className="flex flex-col gap-1">
              <span className="font-medium">This is going to be slow, and it is on you.</span>
              <span>
                On macOS and Windows a bind mount is the single biggest performance problem
                a hub can have: database migrations that take seconds on a volume take
                minutes here, and image uploads crawl. On Linux the folder ends up owned by
                root, so deleting it later needs Konstruktor's help. Pick this only if you
                need the data as a folder and know what you are giving up — the default can
                still be backed up to a folder at any time.
              </span>
            </div>
          </Alert>
        )}
      </div>
    </StepFrame>
  );
};
