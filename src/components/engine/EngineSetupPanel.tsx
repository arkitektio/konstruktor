import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open } from "@tauri-apps/plugin-shell";
import {
  Check,
  ChevronDown,
  CircleCheck,
  Copy,
  Download,
  ExternalLink,
  Loader2,
  Play,
  RefreshCw,
  TriangleAlert,
  X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "../../api";
import type {
  DockerProbe,
  DockerState,
  EngineBrand,
  InstallLine,
  InstallOutcome,
  InstallerId,
  Remedy,
  RemedyStep,
  StartTarget,
} from "../../api";
import { useCommunication } from "../../communication/communication-context";
import { cn } from "../../utils";
import { Alert } from "../ui/alert";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "../ui/collapsible";

/**
 * Everything about the container engine, in one place: the verdict, what was found, and
 * what to do about it — as the core worded it, for this OS and this product.
 *
 * The remedies are data from `konstruktor-core::remedy`. This panel renders each step as
 * the right control — a link, a code block, an install button with a live log, a start
 * button — and adds no advice of its own, so `konstruktor doctor` and the app never say
 * different things. It is the same panel everywhere: the wizard's first step, the dialog
 * behind the status dot, and the dashboard of a deployment whose engine went away.
 */

/** What to call the product, when the probe could tell. */
const BRAND_LABEL: Record<EngineBrand, string | null> = {
  "docker-desktop": "Docker Desktop",
  colima: "Colima",
  "orb-stack": "OrbStack",
  "rancher-desktop": "Rancher Desktop",
  "podman-desktop": "Podman",
  native: null,
  unknown: null,
};

type Tone = "ok" | "warn" | "bad";

type Verdict = {
  tone: Tone;
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  body: string;
};

/**
 * The headline for each state. The name is the product where we know it and the engine
 * otherwise: "Colima is installed, but not running" is an instruction, "Docker is not
 * running" on a Colima machine is a puzzle.
 */
const verdict = (probe: DockerProbe, state: Exclude<DockerState, "checking">): Verdict => {
  const engine = api.engineName(probe.engine);
  const product = BRAND_LABEL[probe.brand] ?? engine;
  switch (state) {
    case "ready":
      return {
        tone: "ok",
        icon: CircleCheck,
        title: `${product} is ready`,
        body: "The command line, the compose plugin and the daemon all answered. Konstruktor can write a deployment and start it.",
      };
    case "missing":
      return {
        tone: "bad",
        icon: Download,
        title: "No container engine is installed",
        body: "Konstruktor hands the finished deployment to Docker Compose, so a Docker-compatible engine has to be on this machine. It is the only thing you need to install — there is no Python and no CLI to set up.",
      };
    case "no-compose":
      return {
        tone: "bad",
        icon: TriangleAlert,
        title: `${engine} is installed, but Compose is missing`,
        body: "The command line works, but `compose` does not answer. It ships as a plugin beside the CLI, and it is not there.",
      };
    case "no-daemon":
      return {
        tone: "warn",
        icon: Play,
        title: `${product} is installed, but not running`,
        body: `The command line is there, but nothing is answering at the daemon. Start ${product} and this page will notice by itself.`,
      };
    case "too-old":
      return {
        tone: "warn",
        icon: TriangleAlert,
        title: `${product} is too old`,
        body: "Everything answers, but the Compose or engine version predates what the generated stacks rely on.",
      };
  }
};

const TONE = {
  ok: "border-primary/60 bg-primary/5",
  warn: "border-warning/60 bg-warning/10",
  bad: "border-destructive/60 bg-destructive/5",
} as const;

const ICON_TONE = {
  ok: "text-primary",
  warn: "text-warning",
  bad: "text-destructive",
} as const;

/** The individual findings, so "it says no" is never the whole answer. */
const Findings = ({ probe }: { probe: DockerProbe }) => (
  <div className="flex flex-wrap gap-1.5 mt-3">
    <Finding ok={probe.cli} label={probe.engine ?? "docker"} detail={probe.cli_version} />
    <Finding ok={probe.compose} label="compose" detail={probe.compose_version} />
    <Finding
      ok={probe.daemon}
      label="daemon"
      detail={probe.api_version && `API ${probe.api_version}`}
    />
    {BRAND_LABEL[probe.brand] && (
      <Badge variant="outline" className="font-normal text-muted-foreground">
        {BRAND_LABEL[probe.brand]}
      </Badge>
    )}
  </div>
);

const Finding = ({
  ok,
  label,
  detail,
}: {
  ok: boolean;
  label: string;
  detail?: string | null;
}) => (
  <Badge
    variant="outline"
    className={cn("gap-1 font-normal", ok ? "text-foreground" : "text-muted-foreground")}
  >
    <span
      className={cn("size-1.5 rounded-full", ok ? "bg-primary" : "bg-muted-foreground/40")}
    />
    {label}
    {ok && detail ? <span className="text-muted-foreground">{detail}</span> : null}
  </Badge>
);

// --- the install log ---------------------------------------------------------------

type Install = {
  installer: InstallerId;
  lines: InstallLine[];
  outcome: InstallOutcome | null;
  error: string | null;
};

const InstallLog = ({ install, onCancel }: { install: Install; onCancel: () => void }) => {
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [install.lines.length]);

  const running = install.outcome === null && install.error === null;

  return (
    <div className="mt-3 rounded-md border border-border bg-muted/40 text-xs">
      <div className="flex items-center gap-2 px-3 py-1.5 border-b border-border/60">
        {running ? (
          <Loader2 className="size-3.5 animate-spin text-muted-foreground" />
        ) : install.outcome?.ok ? (
          <CircleCheck className="size-3.5 text-primary" />
        ) : (
          <TriangleAlert className="size-3.5 text-destructive" />
        )}
        <span className="font-medium">
          {running
            ? "Installing…"
            : install.outcome?.ok
              ? install.outcome.needsReboot
                ? "Installed — Windows needs to restart"
                : "Installed"
              : install.outcome?.cancelled
                ? "Cancelled"
                : (install.outcome?.message ?? install.error ?? "Failed")}
        </span>
        {running && (
          <Button size="xs" variant="ghost" className="ml-auto" onClick={onCancel}>
            <X className="size-3" />
            Cancel
          </Button>
        )}
      </div>
      <div className="max-h-56 overflow-auto px-3 py-2 font-mono whitespace-pre-wrap break-all">
        {install.lines.map((line, i) =>
          line.stage ? (
            <div key={i} className="mt-2 first:mt-0 font-sans font-medium text-foreground">
              {line.line}
            </div>
          ) : (
            <div
              key={i}
              className={line.stderr ? "text-muted-foreground" : "text-foreground/80"}
            >
              {line.line}
            </div>
          )
        )}
        <div ref={end} />
      </div>
      {install.outcome?.needsReboot && (
        <div className="px-3 py-2 border-t border-border/60 text-muted-foreground">
          The installer asked for a restart before the engine can start. Restart
          Windows, open Konstruktor again, and it will pick up from here.
        </div>
      )}
    </div>
  );
};

// --- the steps -----------------------------------------------------------------------

const CopyCommand = ({ label, command }: { label: string; command: string }) => {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await writeText(command);
    } catch {
      // Outside the Tauri window there is no clipboard plugin. The text is still on
      // screen to select by hand.
    }
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div className="mt-2">
      <div className="text-xs text-muted-foreground mb-1">{label}</div>
      <div className="flex items-start gap-1.5">
        <code className="flex-1 min-w-0 rounded-md border border-border bg-muted/40 px-2.5 py-1.5 text-xs font-mono whitespace-pre-wrap break-all select-all">
          {command}
        </code>
        <Button size="icon-sm" variant="ghost" onClick={copy} aria-label="Copy">
          {copied ? <Check className="size-3.5 text-primary" /> : <Copy className="size-3.5" />}
        </Button>
      </div>
    </div>
  );
};

const StepView = ({
  step,
  install,
  onInstall,
  onStart,
  starting,
  busy,
}: {
  step: RemedyStep;
  install: Install | null;
  onInstall: (installer: InstallerId) => void;
  onStart: (target: StartTarget) => void;
  starting: StartTarget | null;
  busy: boolean;
}) => {
  switch (step.kind) {
    case "open-url":
      return (
        <Button size="sm" variant="outline" onClick={() => open(step.url)}>
          <ExternalLink className="size-3.5" />
          {step.label}
        </Button>
      );
    case "copy-command":
      return <CopyCommand label={step.label} command={step.command} />;
    case "run-installer": {
      const running =
        install?.installer === step.installer &&
        install.outcome === null &&
        install.error === null;
      return (
        <Button
          size="sm"
          disabled={busy && !running}
          onClick={() => onInstall(step.installer)}
        >
          {running ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <Download className="size-3.5" />
          )}
          {running ? "Installing…" : step.label}
        </Button>
      );
    }
    case "start-engine":
      return (
        <Button size="sm" disabled={busy} onClick={() => onStart(step.target)}>
          {starting === step.target ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <Play className="size-3.5" />
          )}
          {starting === step.target ? "Starting…" : step.label}
        </Button>
      );
    case "note":
      return <p className="text-xs text-muted-foreground leading-relaxed">{step.text}</p>;
  }
};

const RemedyCard = ({
  remedy,
  ...rest
}: {
  remedy: Remedy;
  install: Install | null;
  onInstall: (installer: InstallerId) => void;
  onStart: (target: StartTarget) => void;
  starting: StartTarget | null;
  busy: boolean;
}) => {
  // Buttons sit in a row; commands and notes each take their own line.
  const inline = remedy.steps.filter(
    (s) => s.kind === "open-url" || s.kind === "run-installer" || s.kind === "start-engine"
  );
  const block = remedy.steps.filter((s) => s.kind === "copy-command" || s.kind === "note");
  const shown =
    rest.install && inline.some((s) => s.kind === "run-installer" && s.installer === rest.install!.installer)
      ? rest.install
      : null;

  return (
    <Card
      className={cn(
        "gap-0 py-4 px-4 border",
        remedy.primary ? "border-primary/40" : "border-border"
      )}
    >
      <div className="flex items-center gap-2">
        <span className="font-medium text-sm">{remedy.title}</span>
        {remedy.primary && (
          <Badge variant="outline" className="font-normal text-primary border-primary/40">
            Recommended
          </Badge>
        )}
      </div>
      <p className="text-sm text-muted-foreground mt-1 leading-relaxed">{remedy.body}</p>
      {inline.length > 0 && (
        <div className="flex flex-wrap items-center gap-2 mt-3">
          {inline.map((step, i) => (
            <StepView key={i} step={step} {...rest} />
          ))}
        </div>
      )}
      {shown && <InstallLog install={shown} onCancel={() => void api.cancelInstall()} />}
      {block.map((step, i) => (
        <StepView key={i} step={step} {...rest} />
      ))}
    </Card>
  );
};

// --- the panel -----------------------------------------------------------------------

export const EngineSetupPanel = ({
  /** Leave out the alternatives and the findings — for the small dialog. */
  compact = false,
}: {
  compact?: boolean;
}) => {
  const { probe, state, checking, recheck } = useCommunication();
  const [install, setInstall] = useState<Install | null>(null);
  const [starting, setStarting] = useState<StartTarget | null>(null);
  const [showAlternatives, setShowAlternatives] = useState(false);

  const installing = install !== null && install.outcome === null && install.error === null;

  const runInstaller = useCallback(
    async (installer: InstallerId) => {
      setInstall({ installer, lines: [], outcome: null, error: null });
      try {
        const outcome = await api.installEngine(installer, (line) =>
          setInstall((current) =>
            current && current.installer === installer
              ? { ...current, lines: [...current.lines, line] }
              : current
          )
        );
        setInstall((current) => (current ? { ...current, outcome } : current));
      } catch (e) {
        setInstall((current) =>
          current ? { ...current, error: e instanceof Error ? e.message : String(e) } : current
        );
      }
      // Whatever happened, the machine is different now. Look again at once rather than
      // waiting for the poll.
      void recheck();
    },
    [recheck]
  );

  const startTarget = useCallback(
    async (target: StartTarget) => {
      setStarting(target);
      try {
        await api.startEngine(target);
      } catch (e) {
        setInstall({
          installer: "brew-colima",
          lines: [],
          outcome: null,
          error: e instanceof Error ? e.message : String(e),
        });
      }
      // A VM takes a while to come up; the provider keeps polling until it does. The
      // spinner stays on the button meanwhile.
      setTimeout(() => setStarting(null), 20_000);
    },
    []
  );

  // Once it is ready, the spinner has nothing left to say.
  useEffect(() => {
    if (state === "ready") setStarting(null);
  }, [state]);

  const v = probe && state !== "checking" ? verdict(probe, state) : null;
  const Icon = v?.icon ?? Loader2;
  const remedies = probe?.remedies ?? [];
  const [primary, ...alternatives] = remedies;

  return (
    <div className="flex flex-col gap-3">
      <Card
        className={cn("gap-0 py-5 border transition-colors", v ? TONE[v.tone] : "border-border")}
      >
        <div className="px-5 flex items-start gap-3">
          <Icon
            className={cn(
              "size-5 shrink-0 mt-0.5",
              v ? ICON_TONE[v.tone] : "animate-spin text-muted-foreground"
            )}
          />
          <div className="min-w-0 flex-1">
            <div className="font-semibold">{v?.title ?? "Looking for a container engine…"}</div>
            <p className="text-sm text-muted-foreground mt-1 leading-relaxed">
              {v?.body ??
                "Checking whether the Docker or Podman command line, the compose plugin and the daemon are all here."}
            </p>

            {probe && !compact && <Findings probe={probe} />}

            {state !== "ready" && state !== "checking" && (
              <div className="flex flex-wrap items-center gap-2 mt-4">
                <Button
                  size="sm"
                  variant="outline"
                  disabled={checking}
                  onClick={() => void recheck()}
                >
                  <RefreshCw className={cn("size-3.5", checking && "animate-spin")} />
                  {checking ? "Checking…" : "Check again"}
                </Button>
                <span className="text-xs text-muted-foreground">
                  Konstruktor keeps looking on its own, too.
                </span>
              </div>
            )}
          </div>
        </div>
      </Card>

      {state === "no-daemon" && probe?.error && !compact && (
        <Alert className="text-xs text-muted-foreground">{probe.error}</Alert>
      )}

      {primary && (
        <RemedyCard
          remedy={primary}
          install={install}
          onInstall={runInstaller}
          onStart={startTarget}
          starting={starting}
          busy={installing}
        />
      )}

      {alternatives.length > 0 && (
        <Collapsible open={showAlternatives} onOpenChange={setShowAlternatives}>
          <CollapsibleTrigger asChild>
            <Button variant="ghost" size="sm" className="text-muted-foreground">
              <ChevronDown
                className={cn("size-3.5 transition-transform", showAlternatives && "rotate-180")}
              />
              {showAlternatives ? "Fewer options" : `Other options (${alternatives.length})`}
            </Button>
          </CollapsibleTrigger>
          <CollapsibleContent className="flex flex-col gap-2 mt-2">
            {alternatives.map((remedy) => (
              <RemedyCard
                key={remedy.title}
                remedy={remedy}
                install={install}
                onInstall={runInstaller}
                onStart={startTarget}
                starting={starting}
                busy={installing}
              />
            ))}
          </CollapsibleContent>
        </Collapsible>
      )}
    </div>
  );
};
