import { Link } from "react-router-dom";

import { Button } from "../components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../components/ui/card";
import { Logo } from "../layout/Logo";
import { Page } from "../layout/Page";
import { useStorage } from "../storage/storage-context";

export const Home: React.FC<{}> = (props) => {
  const { apps, deleteApp } = useStorage();

  return (
    <Page
      buttons={
        <>
          <Button asChild>
            <Link to="/setup">Create New Deployment</Link>
          </Button>
        </>
      }
    >
      {apps.length > 0 ? (
        <div>
          <CardHeader>
            <CardTitle>Your deployments:</CardTitle>
            <CardDescription>
              Click on an app to view its dashboard
            </CardDescription>
          </CardHeader>
          <CardContent>
            <div className="flex flex-row flex-wrap gap-2">
              {apps.map((app, index) => (
                <Card className="max-w-sm">
                  <CardHeader>
                    <div className="mb-2">
                      <Logo
                        width={"40"}
                        height={"40"}
                        cubeColor={"hsl(var(--accent)"}
                        aColor={"hsl(var(--foreground)"}
                        strokeColor={"hsl(var(--foreground)"}
                      />
                    </div>

                    <CardTitle>{app.name}</CardTitle>
                    <CardDescription className="truncate">
                      {app.path}
                    </CardDescription>
                  </CardHeader>
                  <CardContent>
                    <Button key={index} asChild>
                      <Link to={`/dashboard/${app.name}`}>Open App</Link>
                    </Button>
                  </CardContent>
                </Card>
              ))}
            </div>
          </CardContent>
        </div>
      ) : (
        <div className="items-start flex flex-col">
          <div className="mb-2">
            <Logo
              width={"120"}
              height={"120"}
              cubeColor={"hsl(var(--accent)"}
              aColor={"hsl(var(--foreground)"}
              strokeColor={"hsl(var(--foreground)"}
            />
          </div>
          <div className="text-2xl font-bold">Welcome to Konstruktor</div>
          <div className="text-xl font-light mt-2 max-w-xl">
            You don't have any deployments yet. We are excited to see what you
            will build and use Arkitekt for.
          </div>
          <div className="text-xs text-muted-foreground font-light mt-2 max-w-xl">
            With Konstruktor you can easily create and manage your own deployments of Arkitekt
            and other modern bioimage apps. Just click the button below to get started.
          </div>
        </div>
      )}
    </Page>
  );
};
