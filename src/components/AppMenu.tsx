import { Cog } from "lucide-react";
import { Link, useNavigate } from "react-router-dom";
import { useCommunication } from "../communication/communication-context";
import { Logo } from "../layout/Logo";
import { cn } from "../utils";
import { Button } from "./ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "./ui/tooltip";

/**
 * The bar across the top of every screen outside the wizard.
 *
 * It used to be a menubar whose menus held one item each; what people actually reached
 * for was "go home" and "settings", so those are buttons now and the bar reads like the
 * wizard's header — logo, where you are, a small right-hand side.
 */

/** What the Docker dot means, when it is not green. */
const DOCKER_SUMMARY: Record<string, string> = {
  checking: "Looking for Docker…",
  ready: "Docker is ready",
  missing: "Docker is not installed",
  "no-compose": "Docker is here, but the compose plugin is missing",
  "no-daemon": "Docker is installed, but not running",
};

export const DockerDot = () => {
  const { state, probe } = useCommunication();

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <div className="flex items-center gap-1.5 px-1.5 cursor-default">
          <span
            className={cn(
              "size-2 rounded-full",
              state === "ready"
                ? "bg-primary"
                : state === "checking"
                  ? "bg-muted-foreground/40 animate-pulse"
                  : "bg-warning"
            )}
          />
          <span className="text-xs text-muted-foreground">Docker</span>
        </div>
      </TooltipTrigger>
      <TooltipContent>
        {DOCKER_SUMMARY[state]}
        {probe?.cli_version ? ` · ${probe.cli_version}` : ""}
      </TooltipContent>
    </Tooltip>
  );
};

export const AppMenu = ({
  /** What this screen is, shown after the app name. */
  breadcrumb,
  /** Screen-specific controls, before the settings button. */
  actions,
}: {
  breadcrumb?: React.ReactNode;
  actions?: React.ReactNode;
} = {}) => {
  const navigate = useNavigate();

  return (
    <div className="flex items-center gap-2 px-4 py-2.5 border-b border-border/60">
      <button
        type="button"
        onClick={() => navigate("/")}
        className="flex items-center gap-2 rounded-md px-1.5 py-1 -mx-1.5 hover:bg-accent/50 transition-colors"
      >
        <Logo
          width={20}
          height={20}
          aColor="currentColor"
          strokeColor="currentColor"
        />
        <span className="text-sm font-semibold tracking-tight">Konstruktor</span>
      </button>

      {breadcrumb && (
        <>
          <span className="text-muted-foreground/50 text-sm">/</span>
          <span className="text-sm text-muted-foreground truncate min-w-0">
            {breadcrumb}
          </span>
        </>
      )}

      <div className="flex-1" />

      <DockerDot />
      {actions}

      <Tooltip>
        <TooltipTrigger asChild>
          <Button variant="ghost" size="icon-sm" asChild>
            <Link to="/settings" aria-label="Settings">
              <Cog className="size-4" />
            </Link>
          </Button>
        </TooltipTrigger>
        <TooltipContent>Settings</TooltipContent>
      </Tooltip>
    </div>
  );
};
