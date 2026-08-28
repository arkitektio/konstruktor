import { open } from "@tauri-apps/plugin-shell";
import {
  CircleCheck,
  Download,
  ExternalLink,
  Loader2,
  Play,
  RefreshCw,
  TriangleAlert,
} from "lucide-react";
import { useEffect } from "react";
import { useFormContext } from "react-hook-form";
import type { DockerProbe, DockerState } from "../../../api";
import { useCommunication } from "../../../communication/communication-context";
import { ErrorDisplay } from "../../../components/Error";
import { Alert } from "../../../components/ui/alert";
import { Badge } from "../../../components/ui/badge";
import { Button } from "../../../components/ui/button";
import { Card } from "../../../components/ui/card";
import { cn } from "../../../utils";
import { StepFrame } from "../../wizard/StepFrame";

/**
 * Where the Docker install instructions live, per platform. `plugin-os` reads a value
 * the shell injects, which is absent outside the Tauri window (tests, `vite dev` in a
 * browser), so the lookup is guarded and falls back to the generic page.
 */
const INSTALL_DOCS: Record<string, string> = {
  macos: "https://docs.docker.com/desktop/setup/install/mac-install/",
  windows: "https://docs.docker.com/desktop/setup/install/windows-install/",
  linux: "https://docs.docker.com/engine/install/",
};

const GENERIC_DOCS = "https://docs.docker.com/get-started/get-docker/";

const installDocs = (): string => {
  try {
    // Read straight off the injected global rather than through `platform()`, which
    // throws when the global is absent — outside the Tauri window we just want the
    // generic page, not an exception on a cosmetic lookup.
    const platform = (window as any).__TAURI_OS_PLUGIN_INTERNALS__?.platform;
    return INSTALL_DOCS[platform] ?? GENERIC_DOCS;
  } catch {
    return GENERIC_DOCS;
  }
};

type Verdict = {
  tone: "ok" | "warn" | "bad";
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  body: string;
  /** The one thing to do about it, when there is one. */
  action?: { label: string; url: string; icon: React.ComponentType<{ className?: string }> };
};

/**
 * Each way this can go has its own remedy — that is the whole point of splitting the
 * probe into `cli` / `compose` / `daemon` rather than one boolean. Sending somebody
 * whose Docker is merely stopped to a download page wastes a download and their time.
 */
const VERDICTS: Record<Exclude<DockerState, "checking">, Verdict> = {
  ready: {
    tone: "ok",
    icon: CircleCheck,
    title: "Docker is ready",
    body: "The CLI, the compose plugin and the daemon all answered. Konstruktor can write this deployment and start it for you.",
  },
  missing: {
    tone: "bad",
    icon: Download,
    title: "Docker is not installed",
    body: "Konstruktor hands the finished deployment to Docker Compose, so Docker has to be on this machine. It is the only thing you need to install — there is no Python and no CLI to set up. Install it, then come back and check again.",
    action: { label: "Install Docker", url: installDocs(), icon: Download },
  },
  "no-compose": {
    tone: "bad",
    icon: TriangleAlert,
    title: "Docker is installed, but Compose is missing",
    body: "The `docker` command works, but `docker compose` does not. Compose ships as a plugin with current Docker versions — installing a recent Docker Desktop or the compose plugin fixes this.",
    action: {
      label: "How to install Compose",
      url: "https://docs.docker.com/compose/install/",
      icon: ExternalLink,
    },
  },
  "no-daemon": {
    tone: "warn",
    icon: Play,
    title: "Docker is installed, but not running",
    body: "The command line is there, but the daemon is not answering. Start Docker Desktop (or the docker service) and check again — the stack cannot be started until it responds.",
  },
};

const TONE = {
  ok: "border-primary/60 bg-primary/5",
  warn: "border-amber-500/60 bg-amber-500/5",
  bad: "border-destructive/60 bg-destructive/5",
} as const;

const ICON_TONE = {
  ok: "text-primary",
  warn: "text-amber-500",
  bad: "text-destructive",
} as const;

/** The individual findings, so "it says no" is never the whole answer. */
const Findings = ({ probe }: { probe: DockerProbe }) => (
  <div className="flex flex-wrap gap-1.5 mt-3">
    <Finding ok={probe.cli} label="docker" detail={probe.cli_version} />
    <Finding ok={probe.compose} label="compose" detail={probe.compose_version} />
    <Finding ok={probe.daemon} label="daemon" detail={probe.api_version && `API ${probe.api_version}`} />
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
      className={cn(
        "size-1.5 rounded-full",
        ok ? "bg-primary" : "bg-muted-foreground/40"
      )}
    />
    {label}
    {ok && detail ? <span className="text-muted-foreground">{detail}</span> : null}
  </Badge>
);

/**
 * The first thing the wizard asks, and the only question it answers by itself.
 *
 * The probe runs on mount rather than relying on the one the app made at startup: the
 * expected path through a failure here is "leave, install Docker, come back", and a
 * cached answer from before that would be wrong.
 */
export const DockerStep = () => {
  const { probe, state, checking, recheck } = useCommunication();
  const { setValue } = useFormContext();

  useEffect(() => {
    recheck();
  }, [recheck]);

  useEffect(() => {
    setValue("dockerOk", state === "ready", { shouldValidate: true });
  }, [state, setValue]);

  const verdict = state === "checking" ? undefined : VERDICTS[state];
  const Icon = verdict?.icon ?? Loader2;

  return (
    <StepFrame
      title="Docker"
      subtitle="The one thing this machine has to have"
      lead="Konstruktor writes the deployment itself and hands it to Docker Compose to run. Nothing else gets installed on your system — no Python, no CLI, no helper container."
    >
      <Card
        className={cn(
          "gap-0 py-5 border transition-colors",
          verdict ? TONE[verdict.tone] : "border-border"
        )}
      >
        <div className="px-5 flex items-start gap-3">
          <Icon
            className={cn(
              "size-5 shrink-0 mt-0.5",
              verdict ? ICON_TONE[verdict.tone] : "animate-spin text-muted-foreground"
            )}
          />
          <div className="min-w-0 flex-1">
            <div className="font-semibold">
              {verdict?.title ?? "Looking for Docker…"}
            </div>
            <p className="text-sm text-muted-foreground mt-1 leading-relaxed">
              {verdict?.body ??
                "Checking whether the Docker command line, the compose plugin and the daemon are all here."}
            </p>

            {probe && <Findings probe={probe} />}

            <div className="flex flex-wrap items-center gap-2 mt-4">
              {verdict?.action && (
                <Button size="sm" onClick={() => open(verdict.action!.url)}>
                  <verdict.action.icon className="size-3.5" />
                  {verdict.action.label}
                </Button>
              )}
              {state !== "ready" && (
                <Button
                  size="sm"
                  variant={verdict?.action ? "outline" : "default"}
                  disabled={checking}
                  onClick={() => recheck()}
                >
                  <RefreshCw className={cn("size-3.5", checking && "animate-spin")} />
                  {checking ? "Checking…" : "Check again"}
                </Button>
              )}
            </div>
          </div>
        </div>
      </Card>

      {state === "no-daemon" && probe?.error && (
        <Alert className="mt-3 text-xs text-muted-foreground">{probe.error}</Alert>
      )}

      <ErrorDisplay name="dockerOk" className="mt-3" />
    </StepFrame>
  );
};
