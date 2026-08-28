import { useState } from "react";

import * as api from "../../api";
import { Alert } from "../../components/ui/alert";
import { Button } from "../../components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "../../components/ui/dialog";
import { Input } from "../../components/ui/input";

/**
 * An admin account for one service, made once the service is running.
 *
 * The wizard used to ask for a username and a password before anything existed, and the
 * answer was written into every service's config. That is the wrong moment and the wrong
 * shape: each service keeps its own database and its own Django admin, so an account is
 * per service, and it can only be made in a container that is up with its migrations
 * applied. So it is asked here instead — `docker compose exec`, against this one service.
 */
export const SuperuserDialog = ({
  open,
  onOpenChange,
  path,
  service,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The deployment folder — compose runs with it as the working directory. */
  path: string;
  /** The compose service to make the account in. */
  service: string;
}) => {
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("");
  const [email, setEmail] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);

  const submit = async () => {
    setRunning(true);
    setError(null);
    try {
      await api.createSuperuser(path, service, username.trim(), password, email.trim());
      setDone(true);
      setPassword("");
    } catch (failure) {
      // Django's own complaint — "that username is already taken" is the common one, and
      // it is far more use than anything this dialog could say instead.
      setError(typeof failure === "string" ? failure : String(failure));
    } finally {
      setRunning(false);
    }
  };

  const ready = username.trim().length > 0 && password.length > 0 && !running;

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        onOpenChange(next);
        if (!next) {
          setDone(false);
          setError(null);
          setPassword("");
        }
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>An admin account for {service}</DialogTitle>
          <DialogDescription>
            Creates a Django superuser inside the running {service} container, for its
            own admin site. Each service has a separate one — this account does not
            exist in the others.
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-3">
          <label className="flex flex-col gap-1.5">
            <span className="text-sm font-medium">Username</span>
            <Input
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              autoComplete="off"
              spellCheck={false}
            />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-sm font-medium">Password</span>
            <Input
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              autoComplete="new-password"
            />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-sm font-medium">Email</span>
            <Input
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              placeholder="optional"
              autoComplete="off"
              spellCheck={false}
            />
          </label>

          {error && (
            <Alert variant="destructive" className="text-xs whitespace-pre-wrap">
              {error}
            </Alert>
          )}
          {done && !error && (
            <Alert className="text-xs">
              {username.trim()} can now sign in to {service}'s admin site.
            </Alert>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {done ? "Close" : "Cancel"}
          </Button>
          <Button disabled={!ready} onClick={() => void submit()}>
            {running ? "Creating…" : "Create"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
