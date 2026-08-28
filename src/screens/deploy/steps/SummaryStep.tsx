import { Rocket } from "lucide-react";
import { StepFrame } from "../../wizard/StepFrame";

export type SummaryRow = { label: string; value: string };

/**
 * The last look before anything leaves the app. Nothing has been written and nothing has
 * been sent to the coordination server yet at this point — the next step does both.
 */
export const SummaryStep = ({
  title = "Ready",
  subtitle = "This is what will be created",
  rows,
  files,
}: {
  title?: string;
  subtitle?: string;
  rows: SummaryRow[];
  /** The files that will be written into the folder, for the "no surprises" list. */
  files?: string[];
}) => (
  <StepFrame icon={Rocket} title={title} subtitle={subtitle}>
    <div className="max-w-2xl rounded-lg border border-border bg-card divide-y divide-border">
      {rows
        .filter((row) => row.value !== "")
        .map((row) => (
          <div
            key={row.label}
            className="grid grid-cols-3 gap-3 px-4 py-2.5 text-sm"
          >
            <div className="text-muted-foreground">{row.label}</div>
            <div className="col-span-2 break-all">{row.value}</div>
          </div>
        ))}
    </div>

    {files && files.length > 0 && (
      <div className="max-w-2xl mt-6">
        <div className="text-sm font-medium mb-2">Konstruktor will write</div>
        <div className="bg-card border border-border rounded-lg p-3 flex flex-col gap-0.5">
          {files.map((file) => (
            <code key={file} className="text-xs break-all text-muted-foreground">
              {file}
            </code>
          ))}
        </div>
      </div>
    )}
  </StepFrame>
);
