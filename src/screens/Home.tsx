import { ArrowRight, Boxes, Plus, Puzzle, TriangleAlert } from "lucide-react";
import { Link } from "react-router-dom";
import { useCommunication } from "../communication/communication-context";
import { Button } from "../components/ui/button";
import { Card } from "../components/ui/card";
import { Logo } from "../layout/Logo";
import { Page } from "../layout/Page";
import type { DeploymentRecord } from "../api";
import { useRegistry } from "../registry/registry-context";
import { DeploymentPaths } from "./deploy/NewDeployment";

/** What Home says about Docker when it is not ready. Silent when it is. */
const DOCKER_WARNING: Record<string, string> = {
  missing: "Docker is not installed. A new deployment cannot be started without it.",
  "no-compose": "Docker is here, but the compose plugin is missing.",
  "no-daemon": "Docker is installed, but not running. Start it to manage deployments.",
};

/**
 * One kind's deployments, as a column.
 *
 * A column rather than a filtered grid: the two kinds are managed separately and the
 * heading is what says which is which, instead of a badge you have to read on every row.
 */
const Column = ({
  icon: Icon,
  title,
  empty,
  deployments,
  to,
}: {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  empty: string;
  deployments: DeploymentRecord[];
  /** Where "new one of these" goes. */
  to: string;
}) => (
  <div className="flex flex-col gap-2">
    <div className="flex items-center gap-2">
      <Icon className="size-4 text-muted-foreground" />
      <span className="text-sm font-medium">{title}</span>
      <span className="text-xs text-muted-foreground">{deployments.length}</span>
      <Button asChild size="icon-sm" variant="ghost" className="ml-auto" aria-label={`New ${title}`}>
        <Link to={to}>
          <Plus className="size-3.5" />
        </Link>
      </Button>
    </div>

    {deployments.length === 0 ? (
      <div className="text-xs text-muted-foreground border border-dashed border-border rounded-lg px-4 py-6 text-center">
        {empty}
      </div>
    ) : (
      deployments.map((deployment) => (
        <Card
          key={deployment.id}
          className="gap-0 py-3 px-4 border-border hover:border-primary/50 transition-colors"
        >
          <div className="flex items-center gap-3">
            <Logo
              width={26}
              height={26}
              aColor="currentColor"
              strokeColor="currentColor"
            />
            <div className="min-w-0 flex-1">
              <div className="font-medium truncate">{deployment.name}</div>
              <div
                className="text-xs text-muted-foreground truncate"
                title={deployment.path}
              >
                {deployment.path}
              </div>
            </div>
            <Button asChild size="sm" variant="outline">
              <Link to={`/dashboard/${deployment.id}`}>
                Open
                <ArrowRight className="size-3.5" />
              </Link>
            </Button>
          </div>
        </Card>
      ))
    )}
  </div>
);

export const Home: React.FC<{}> = () => {
  const { deployments, loading } = useRegistry();
  const { state } = useCommunication();

  const warning = DOCKER_WARNING[state];
  const engines = deployments.filter((d) => d.kind === "engine");
  const hubs = deployments.filter((d) => d.kind !== "engine");

  return (
    <Page
      buttons={
        <>
          <Button asChild>
            <Link to="/new">
              <Plus className="size-3.5" />
              New
            </Link>
          </Button>
        </>
      }
    >
      {loading ? null : deployments.length > 0 ? (
        <div className="flex flex-col gap-5">
          <div>
            <h1 className="text-2xl font-bold tracking-tight">Your deployments</h1>
            <p className="text-sm text-muted-foreground mt-0.5">
              Open one to start, stop and inspect it.
            </p>
          </div>

          {warning && <DockerWarning message={warning} />}

          {/*
            Split by kind, side by side. A hub and a plugin engine are different things
            you do different work in, and a single list sorted by creation date mixed
            them — with a small grey badge as the only way to tell which was which.
          */}
          <div className="grid grid-cols-1 @3xl:grid-cols-2 gap-6 items-start">
            <Column
              icon={Boxes}
              title="Hubs"
              empty="No hubs yet."
              deployments={hubs}
              to="/new/hub"
            />
            <Column
              icon={Puzzle}
              title="Plugin engines"
              empty="No plugin engines yet."
              deployments={engines}
              to="/new/engine"
            />
          </div>
        </div>
      ) : (
        <div className="flex w-full min-h-[60vh] items-center justify-center">
          <div className="flex flex-col items-center text-center max-w-md gap-5">
            <Logo
              width={110}
              height={110}
              aColor="currentColor"
              strokeColor="currentColor"
            />
            <div className="space-y-2">
              <h1 className="text-3xl font-bold tracking-tight">
                Welcome to Konstruktor
              </h1>
              <p className="text-sm text-muted-foreground">
                Konstruktor creates and manages the two halves of an Arkitekt
                deployment: hubs, which are the data and compute services, and plugin
                engines, which run the plugins an organization installs. Both are
                ordinary Docker Compose projects in a folder you choose.
              </p>
            </div>

            {warning && <DockerWarning message={warning} />}

            <div className="w-full text-left">
              <DeploymentPaths />
            </div>
          </div>
        </div>
      )}
    </Page>
  );
};

const DockerWarning = ({ message }: { message: string }) => (
  <div className="flex items-start gap-2 rounded-lg border border-warning/60 bg-warning/10 px-3 py-2 text-left text-xs">
    <TriangleAlert className="size-3.5 shrink-0 mt-0.5 text-warning" />
    <span className="text-muted-foreground">{message}</span>
  </div>
);
