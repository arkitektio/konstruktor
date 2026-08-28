import { ArrowRight, Plus, TriangleAlert } from "lucide-react";
import { Link } from "react-router-dom";
import { useCommunication } from "../communication/communication-context";
import { Button } from "../components/ui/button";
import { Card } from "../components/ui/card";
import { Badge } from "../components/ui/badge";
import { Logo } from "../layout/Logo";
import { Page } from "../layout/Page";
import { useRegistry } from "../registry/registry-context";

/** What Home says about Docker when it is not ready. Silent when it is. */
const DOCKER_WARNING: Record<string, string> = {
  missing: "Docker is not installed. A new deployment cannot be started without it.",
  "no-compose": "Docker is here, but the compose plugin is missing.",
  "no-daemon": "Docker is installed, but not running. Start it to manage deployments.",
};

export const Home: React.FC<{}> = () => {
  const { deployments, loading } = useRegistry();
  const { state } = useCommunication();

  const warning = DOCKER_WARNING[state];

  return (
    <Page
      buttons={
        <>
          <Button asChild>
            <Link to="/new">
              <Plus className="size-3.5" />
              New deployment
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

          <div className="grid grid-cols-1 @2xl:grid-cols-2 @4xl:grid-cols-3 gap-3">
            {deployments.map((deployment) => (
              <Card
                key={deployment.id}
                className="gap-0 py-4 px-4 border-border hover:border-primary/50 transition-colors"
              >
                <div className="flex items-start gap-3">
                  <Logo
                    width={32}
                    height={32}
                    cubeColor="var(--primary)"
                    aColor="currentColor"
                    strokeColor="currentColor"
                  />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="font-medium truncate">{deployment.name}</span>
                      <Badge variant="outline" className="font-normal text-[10px]">
                        {deployment.kind}
                      </Badge>
                    </div>
                    <div
                      className="text-xs text-muted-foreground truncate mt-0.5"
                      title={deployment.path}
                    >
                      {deployment.path}
                    </div>
                  </div>
                </div>
                <Button asChild size="sm" variant="outline" className="mt-4 self-start">
                  <Link to={`/dashboard/${deployment.id}`}>
                    Open
                    <ArrowRight className="size-3.5" />
                  </Link>
                </Button>
              </Card>
            ))}
          </div>
        </div>
      ) : (
        <div className="flex w-full min-h-[60vh] items-center justify-center">
          <div className="flex flex-col items-center text-center max-w-md gap-5">
            <Logo
              width={110}
              height={110}
              cubeColor="var(--primary)"
              aColor="currentColor"
              strokeColor="currentColor"
            />
            <div className="space-y-2">
              <h1 className="text-3xl font-bold tracking-tight">
                Welcome to Konstruktor
              </h1>
              <p className="text-sm text-muted-foreground">
                Konstruktor creates and manages hubs — the data and compute services that
                make up an Arkitekt deployment. It writes an ordinary Docker Compose
                project into a folder you choose, and runs it from here.
              </p>
            </div>

            {warning && <DockerWarning message={warning} />}

            <Button asChild size="lg">
              <Link to="/new">
                <Plus className="size-4" />
                Create your first hub
              </Link>
            </Button>
          </div>
        </div>
      )}
    </Page>
  );
};

const DockerWarning = ({ message }: { message: string }) => (
  <div className="flex items-start gap-2 rounded-lg border border-amber-500/50 bg-amber-500/5 px-3 py-2 text-left text-xs">
    <TriangleAlert className="size-3.5 shrink-0 mt-0.5 text-amber-500" />
    <span className="text-muted-foreground">{message}</span>
  </div>
);
