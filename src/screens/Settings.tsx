import { ArrowLeft, Cog } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router-dom";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Page } from "../layout/Page";
import { PageHeader } from "../layout/PageHeader";
import { StepField } from "../screens/wizard/StepFrame";
import {
  DEFAULT_COORDINATION_SERVER,
  useSettings,
} from "../settings/settings-context";

/**
 * There is very little to configure: Konstruktor builds the deployment itself now, so
 * the only thing worth remembering between sessions is which coordination server to
 * offer first. Each hub still records the one it was actually authorized against.
 */
export const Settings = () => {
  const { settings, setSettings } = useSettings();
  const [server, setServer] = useState(settings.coordinationServer);
  const [saved, setSaved] = useState(false);

  const save = async () => {
    await setSettings({ ...settings, coordinationServer: server.trim() });
    setSaved(true);
  };

  const dirty = server.trim() !== settings.coordinationServer;

  return (
    <Page
      buttons={
        <>
          <Button disabled={!dirty || server.trim() === ""} onClick={save}>
            Save
          </Button>
          <Button variant="outline" asChild>
            <Link to="/">
              <ArrowLeft className="size-3.5" />
              Back
            </Link>
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-6">
        <PageHeader
          icon={Cog}
          title="Settings"
          subtitle="Global to this installer — deployments keep their own."
        />

        <div className="max-w-xl">
          <StepField
            label="Coordination server"
            hint="The server new hubs are offered first — where accounts, organizations and permissions live. A bare hostname is reached over https."
          >
            <Input
              id="coordination-server"
              value={server}
              onChange={(event) => {
                setServer(event.target.value);
                setSaved(false);
              }}
              placeholder={DEFAULT_COORDINATION_SERVER}
            />
          </StepField>

          <div className="flex items-center gap-3 mt-3">
            {settings.coordinationServer !== DEFAULT_COORDINATION_SERVER && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => setServer(DEFAULT_COORDINATION_SERVER)}
              >
                Use {DEFAULT_COORDINATION_SERVER}
              </Button>
            )}
            {saved && !dirty && (
              <span className="text-xs text-muted-foreground">Saved.</span>
            )}
          </div>
        </div>
      </div>
    </Page>
  );
};
