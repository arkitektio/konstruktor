import { Link } from "react-router-dom";

import { forage } from "@tauri-apps/tauri-forage";
import { useEffect, useState } from "react";
import { useStorage } from "../storage/storage-context";
import { Logo } from "../layout/Logo";
import { Hover } from "../layout/Hover";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../components/ui/card";
import { motion } from "framer-motion";
import { Page } from "../layout/Page";

export const Home: React.FC<{}> = (props) => {
  const { apps, deleteApp } = useStorage();

  return (
<Page>
      <div className="flex-grow p-3 ">
        {apps.length > 0 ? (
          <div>
            <CardHeader>
            <CardTitle>
              Your deployments:
            </CardTitle>
            <CardDescription>
              Click on an app to view its dashboard
            </CardDescription>
            </CardHeader>
            <CardContent>
            <div className="flex flex-row gap-2">
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
                  <CardDescription className="truncate">{app.path}</CardDescription>
                  
                  </CardHeader>
                  <CardContent>
                  
                <Button
                  key={index}
                >
                  <Link to={`/dashboard/${app.name}`}>Open App</Link>
                </Button>
                </CardContent>
                </Card>
              ))}
            </div>
            </CardContent>
          </div>
        ) : (
          <div className="text-center">
            Seems like this is your first time setting things up!
          </div>
        )}
      </div>
      <div className="flex-initial flex flex-row-reverse gap-2 p-3 bg-card  border-t border-foreground">
        <div  className="flex flex-row items-center gap-2">
          <Button>
          <Link
            to="/setup"
          >
            Setup new App
          </Link>
          </Button>
          <Button>
          <Link
            to="/settings"
          >
            Settings
          </Link>
          </Button>
        </div>
      </div>
    </Page>
  );
};
