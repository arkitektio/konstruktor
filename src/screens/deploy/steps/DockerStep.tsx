import { open } from "@tauri-apps/plugin-shell";
import { ExternalLink, GitBranch, RefreshCw } from "lucide-react";
import { useEffect } from "react";
import { useFormContext } from "react-hook-form";
import type { GitProbe } from "../../../api";
import { useCommunication } from "../../../communication/communication-context";
import { ErrorDisplay } from "../../../components/Error";
import { EngineSetupPanel } from "../../../components/engine/EngineSetupPanel";
import { Button } from "../../../components/ui/button";
import { Card } from "../../../components/ui/card";
import { cn } from "../../../utils";
import { StepFrame } from "../../wizard/StepFrame";

/**
 * Git, checked on its own.
 *
 * It is a separate question from Docker with a separate answer: Docker decides whether
 * this wizard can go on at all, git only decides whether one option under Advanced is
 * offered. Folding it into the Docker verdict made a missing git look like a broken
 * prerequisite; here "not found" can be stated plainly and be an acceptable outcome.
 *
 * The re-check button is its own too, because the Docker card hides its own once Docker
 * is ready — which is exactly the state somebody installing git would come back to.
 */
const GitSection = ({
  git,
  checking,
  recheck,
}: {
  git: GitProbe | null;
  checking: boolean;
  recheck: () => void;
}) => {
  const found = git?.cli ?? false;

  return (
    <div className="mt-6">
      <div className="text-sm font-medium">Git</div>
      <p className="text-sm text-muted-foreground mt-0.5 leading-relaxed">
        Optional, and only for one thing: the dev hub option under Advanced, which runs
        the services from a source checkout instead of published images.
      </p>

      <Card className="gap-0 py-4 mt-3 border border-border">
        <div className="px-4 flex items-start gap-3">
          <GitBranch
            className={cn(
              "size-4.5 shrink-0 mt-0.5",
              found ? "text-primary" : "text-muted-foreground"
            )}
          />
          <div className="min-w-0 flex-1">
            <div className="text-sm font-medium">
              {git === null
                ? "Looking for git…"
                : found
                  ? `git is available${git.cli_version ? ` — ${git.cli_version}` : ""}`
                  : "git was not found"}
            </div>
            {git !== null && !found && (
              <>
                <p className="text-sm text-muted-foreground mt-1 leading-relaxed">
                  That is fine for an ordinary hub, which runs published images. It only
                  means the dev hub option is unavailable. Install git and check again to
                  get it back.
                </p>
                <div className="flex flex-wrap items-center gap-2 mt-3">
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => open("https://git-scm.com/downloads")}
                  >
                    <ExternalLink className="size-3.5" />
                    Install git
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={checking}
                    onClick={() => recheck()}
                  >
                    <RefreshCw className={cn("size-3.5", checking && "animate-spin")} />
                    {checking ? "Checking…" : "Check again"}
                  </Button>
                </div>
              </>
            )}
          </div>
        </div>
      </Card>
    </div>
  );
};

/**
 * The first thing the wizard asks, and the only question it answers by itself.
 *
 * The probe runs on mount rather than relying on the one the app made at startup: the
 * expected path through a failure here is "leave, install Docker, come back", and a
 * cached answer from before that would be wrong.
 */
export const DockerStep = () => {
  const { state, checking, recheck, git } = useCommunication();
  const { setValue } = useFormContext();

  useEffect(() => {
    recheck();
  }, [recheck]);

  useEffect(() => {
    setValue("dockerOk", state === "ready", { shouldValidate: true });
  }, [state, setValue]);

  return (
    <StepFrame
      title="Docker"
      subtitle="The one thing this machine has to have"
      lead="Konstruktor writes the deployment itself and hands it to Docker Compose to run. Nothing else gets installed on your system — no Python, no CLI, no helper container."
    >
      <EngineSetupPanel />

      {/* Its own section: a different question, with a different answer, from Docker's. */}
      <GitSection git={git} checking={checking} recheck={recheck} />

      <ErrorDisplay name="dockerOk" className="mt-3" />
    </StepFrame>
  );
};
