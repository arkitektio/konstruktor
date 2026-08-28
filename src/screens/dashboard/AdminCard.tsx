import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { KeyRound } from "lucide-react";
import { useState } from "react";

import { Button } from "../../components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../components/ui/card";
import type { HubStatus } from "../../api";

export const AdminCard = ({ status }: { status: HubStatus }) => {
  const [revealed, setRevealed] = useState(false);

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-3">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted">
            <KeyRound className="size-4" />
          </span>
          Admin account
        </CardTitle>
        <CardDescription>
          The account seeded into every service when the deployment was created, stored
          in the configuration file. Further accounts are made per service — each card
          under Services has an “Admin” button that creates one in that service alone.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        <div className="grid grid-cols-3 text-sm gap-2">
          <div className="text-muted-foreground">Username</div>
          <div className="col-span-2">{status.admin_user}</div>
          <div className="text-muted-foreground">Password</div>
          <div className="col-span-2 break-all font-mono text-xs">
            {revealed ? status.admin_password : "•".repeat(24)}
          </div>
        </div>
        <div className="flex flex-row gap-2">
          <Button variant="outline" onClick={() => setRevealed(!revealed)}>
            {revealed ? "Hide" : "Reveal"}
          </Button>
          <Button
            variant="outline"
            onClick={() => writeText(status.admin_password)}
          >
            Copy password
          </Button>
        </div>
      </CardContent>
    </Card>
  );
};
