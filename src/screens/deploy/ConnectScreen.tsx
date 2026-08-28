import { ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";

import * as api from "../../api";
import type { AdvertisedHost, CreateEvent, HubStatus } from "../../api";
import { AppMenu } from "../../components/AppMenu";
import { Alert } from "../../components/ui/alert";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { Page } from "../../layout/Page";
import { PageHeader, SectionHeading } from "../../layout/PageHeader";
import { useRegistry } from "../../registry/registry-context";
import { HostPicker } from "./HostPicker";
import { InstallProgress, CreateState, emptyCreateState } from "./InstallProgress";
import { StepField } from "../wizard/StepFrame";

/**
 * Authorizing an existing hub again — after moving it to a different network, adding
 * services, or pointing it at another coordination server.
 *
 * It is the same device-code flow the wizard runs, over the profile already on disk. On
 * success the service configs are regenerated, because the JWKS URL the coordination
 * server hands back is what they verify inbound tokens against.
 */
export const ConnectScreen: React.FC<{}> = () => {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { byId, loading } = useRegistry();

  const deployment = id ? byId(id) : undefined;

  const [status, setStatus] = useState<HubStatus | undefined>();
  const [identifier, setIdentifier] = useState("");
  const [selected, setSelected] = useState<AdvertisedHost[]>([]);
  const [candidates, setCandidates] = useState<api.HostCandidate[]>([]);
  const [authorizing, setAuthorizing] = useState<CreateState>(emptyCreateState);

  const server = status?.profile.config.coord_server ?? "";

  useEffect(() => {
    if (!deployment) return;
    api
      .hubStatus(deployment.path)
      .then((s) => {
        setStatus(s);
        setIdentifier(s.identifier ?? deployment.identifier ?? deployment.name);
      })
      .catch(() => undefined);
  }, [deployment]);

  useEffect(() => {
    api
      .hostCandidates()
      .then((found) => {
        setCandidates(found);
        // Pre-tick what the machine recommends; a name is offered but never assumed.
        setSelected(
          found
            .filter((c) => c.recommended)
            .map((c) => ({ host: c.value, kind: c.kind }))
        );
      })
      .catch(() => undefined);
  }, []);

  const toggle = (value: string) => {
    const candidate = candidates.find((c) => c.value === value);
    if (!candidate) return;
    setSelected((current) =>
      current.some((h) => h.host === value)
        ? current.filter((h) => h.host !== value)
        : [...current, { host: value, kind: candidate.kind }]
    );
  };

  const connect = useCallback(async () => {
    if (!deployment) return;
    setAuthorizing({ ...emptyCreateState, running: true });

    const onEvent = (event: CreateEvent) =>
      setAuthorizing((previous) => ({
        ...previous,
        event,
        logs:
          event.event === "writing"
            ? [...previous.logs, `wrote ${event.file}`]
            : previous.logs,
      }));

    try {
      await api.reauthorizeHub(
        {
          path: deployment.path,
          coordServer: server,
          identifier: identifier.trim(),
          hosts: selected,
          // A hub already on the mesh keeps its key; asking again would mint a second.
          requestAuthKey: status?.mesh_hostname == null,
        },
        onEvent
      );
      setAuthorizing((previous) => ({ ...previous, running: false, done: true }));
    } catch (error) {
      setAuthorizing((previous) => ({
        ...previous,
        running: false,
        error: typeof error === "string" ? error : String(error),
      }));
    }
  }, [deployment, server, identifier, selected, status]);

  if (loading) return null;

  if (!deployment) {
    return (
      <Page
        buttons={
          <Button asChild>
            <Link to="/">Home</Link>
          </Button>
        }
      >
        <PageHeader title="Unknown deployment" />
      </Page>
    );
  }

  const busy = authorizing.running;

  return (
    <>
      <InstallProgress
        open={busy || authorizing.done || authorizing.error !== null}
        state={authorizing}
        onClose={() => setAuthorizing(emptyCreateState)}
      />
      <Page
        menu={<AppMenu breadcrumb={`${deployment.name} · authorize`} />}
        buttons={
          <>
            <Button
              disabled={
                selected.length === 0 || busy || !server || identifier.trim() === ""
              }
              onClick={connect}
            >
              {busy ? "Waiting for authorization…" : "Authorize this hub"}
            </Button>
            <Button
              variant="outline"
              onClick={() => navigate(`/dashboard/${deployment.id}`)}
            >
              Back
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-6">
          <div>
            <PageHeader
              icon={ShieldCheck}
              title="Authorize"
              subtitle={`Introduce ${deployment.name} to ${server || "its coordination server"}`}
            />
            <p className="text-sm text-muted-foreground leading-relaxed mt-4 max-w-xl">
              The coordination server needs to know which services this hub runs and where
              they can be reached. Pick the addresses other machines will use, then accept
              the hub in the browser.
            </p>
          </div>

          {authorizing.done && (
            <Alert className="max-w-2xl border-primary/50">
              This hub is authorized and its service configuration has been rewritten.
              Restart the stack from the dashboard to pick it up.
            </Alert>
          )}

          <div className="max-w-xl">
            <StepField
              label="Hub identifier"
              hint="How this hub is known inside the organization. Changing it creates a second hub there rather than updating this one."
            >
              <Input
                id="hub-identifier"
                value={identifier}
                onChange={(event) => setIdentifier(event.target.value)}
              />
            </StepField>
          </div>

          <div>
            <SectionHeading>Addresses</SectionHeading>
            <HostPicker
              candidates={candidates}
              selected={new Set(selected.map((h) => h.host))}
              onToggle={toggle}
              loading={candidates.length === 0}
            />
          </div>

          <div>
            <SectionHeading>What will be advertised</SectionHeading>
            <div className="flex flex-row flex-wrap gap-2 max-w-2xl">
              {(status?.services ?? []).map((service) => (
                <Badge key={service.id} variant="outline">
                  {service.name}
                </Badge>
              ))}
            </div>
          </div>
        </div>
      </Page>
    </>
  );
};
