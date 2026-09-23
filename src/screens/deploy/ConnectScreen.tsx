import { RadioTower, ShieldCheck } from "lucide-react";
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
import { useSettings } from "../../settings/settings-context";
import { HostPicker, ReachChoice, Reachability } from "./HostPicker";
import { defaultPreset, reachFor, selectionFor, toggleHost, widestPreset } from "./reach";
import {
  InstallProgress,
  CreateState,
  emptyCreateState,
  reduceCreate,
} from "./InstallProgress";
import { StepField } from "../wizard/StepFrame";

/**
 * Authorizing an existing hub again — after moving it to a different network, adding
 * services, or pointing it at another coordination server.
 *
 * It is the same device-code flow the wizard runs, over the profile already on disk. On
 * success the service configs are regenerated, because the JWKS URL the coordination
 * server hands back is what they verify inbound tokens against.
 *
 * Unlike the wizard, this screen runs against a hub that may well be up, so it is the one
 * place a reachability probe can come back green — and the only place an alias can
 * honestly be marked public.
 */
export const ConnectScreen: React.FC<{}> = () => {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { byId, loading: registryLoading, refresh } = useRegistry();
  const { settings } = useSettings();

  const deployment = id ? byId(id) : undefined;

  const [status, setStatus] = useState<HubStatus | undefined>();
  const [identifier, setIdentifier] = useState("");
  const [selected, setSelected] = useState<AdvertisedHost[]>([]);
  const [candidates, setCandidates] = useState<api.HostCandidate[]>([]);
  const [presets, setPresets] = useState<api.ReachPreset[]>([]);
  const [reach, setReach] = useState<ReachChoice>("custom");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [reachability, setReachability] = useState<Record<string, Reachability>>({});
  const [probing, setProbing] = useState(false);
  /** This hub predates konstruktor recording what it advertised. */
  const [forgotten, setForgotten] = useState(false);
  const [authorizing, setAuthorizing] = useState<CreateState>(emptyCreateState);

  /** Ask for a new mesh key even though the hub is on a mesh — its last one expired. */
  const [freshKey, setFreshKey] = useState(false);
  const [outcome, setOutcome] = useState<api.ReauthorizeOutcome | undefined>();

  const server = status?.profile.config.coord_server ?? "";
  const ssl = status?.profile.config.gateway.ssl ?? false;
  const mesh = status?.profile.config.mesh;
  const onMesh = Boolean(mesh?.enabled);
  // A mesh-only hub advertises nothing on this machine's networks: the manifest carries
  // its tailnet node and in-network gateway itself, so there are no addresses to pick.
  const meshOnly = onMesh && Boolean(mesh?.mesh_only);
  // From the core, which is where the manifest works it out — a probe has to aim at the
  // socket the coordination server will actually hand out, not a second guess at it.
  const port = status?.advertised_port ?? (ssl ? 443 : 80);

  /**
   * Status and candidates together, because seeding depends on both.
   *
   * Fetched as one so the selection is decided once. Separately, the two resolve in
   * either order and whichever lands second overwrites what the first had seeded.
   */
  useEffect(() => {
    if (!deployment) return;
    let cancelled = false;

    const discovery = Promise.all([api.hubStatus(deployment.path), api.hostCandidates()]);

    discovery
      .then(([status, discovery]) => {
        if (cancelled) return;
        setStatus(status);
        setIdentifier(status.identifier ?? deployment.identifier ?? deployment.name);
        setCandidates(discovery.candidates);
        setPresets(discovery.presets);

        // What the hub already advertises comes first: a fresh scan would silently drop
        // anything this machine cannot see from where it stands, every time.
        const previous = status.advertised_hosts ?? [];
        if (previous.length > 0) {
          setSelected(previous);
          setReach(reachFor(discovery.presets, previous));
          return;
        }

        // An already-authorized hub with nothing recorded was authorized before
        // konstruktor kept track. Its old selection is unknowable, and the code that made
        // it ticked every real address — public ones included — so opening on the narrow
        // default would drop a public alias the moment somebody pressed Authorize.
        const unrecorded = status.authorized;
        const preset = unrecorded
          ? widestPreset(discovery.presets)
          : defaultPreset(discovery.presets);
        if (preset) {
          setSelected(selectionFor(discovery.candidates, preset));
          setReach(preset.id);
        }
        setForgotten(unrecorded);
      })
      .catch((e) => !cancelled && setError(typeof e === "string" ? e : String(e)))
      .finally(() => !cancelled && setLoading(false));

    return () => {
      cancelled = true;
    };
  }, [deployment]);

  useEffect(() => {
    const endpoint = settings.egressEndpoint?.trim();
    if (!endpoint || candidates.length === 0) return;

    let cancelled = false;
    api
      .egressIdentity(endpoint)
      .then((address) => {
        if (cancelled) return;
        setReachability((current) => ({
          ...current,
          [address]: { ...current[address], egress: true },
        }));
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [settings.egressEndpoint, candidates.length]);

  const toggle = (value: string) => {
    setSelected((current) => toggleHost(current, candidates, value));
    setReach("custom");
  };

  /**
   * Asks the configured prober to connect back to each selected address.
   *
   * Only what is selected: probing is a request to a third party per address, and the
   * ones nobody intends to advertise are nobody's business.
   */
  const checkReachability = useCallback(async () => {
    const prober = settings.proberEndpoint?.trim();
    if (!prober) return;

    setProbing(true);
    try {
      for (const host of selected) {
        const result = await api.probeReachability(prober, host.host, port, ssl);
        setReachability((current) => ({
          ...current,
          [host.host]: { ...current[host.host], probe: result },
        }));
      }
    } finally {
      setProbing(false);
    }
  }, [selected, settings.proberEndpoint, port, ssl]);

  /**
   * Stop waiting for somebody to accept the code. Only the wait can be interrupted; the
   * call comes back with "Cancelled." and the hub is left exactly as it was.
   */
  const stopWaiting = useCallback(async () => {
    setAuthorizing((previous) => ({ ...previous, cancelled: true }));
    await api.cancelAuthorization();
  }, []);

  const connect = useCallback(async () => {
    if (!deployment) return;
    setAuthorizing({ ...emptyCreateState, running: true });

    const onEvent = (event: CreateEvent) =>
      setAuthorizing((previous) => reduceCreate(previous, event));

    try {
      const result = await api.reauthorizeHub(
        {
          path: deployment.path,
          coordServer: server,
          identifier: identifier.trim(),
          // The core ignores these for a mesh-only hub either way.
          hosts: meshOnly ? [] : selected,
          // Only a confirmed probe. Marking an alias public invites the coordination
          // server to health check it, and one it cannot reach would look permanently
          // broken — so matching this machine's egress address is not enough.
          reachableHosts: selected
            .map((host) => host.host)
            .filter((host) => reachability[host]?.probe?.result === "reachable"),
          // The core asks for a key when the hub is not on a mesh yet; "fresh" asks
          // anyway, for a key that expired before the hub ever joined.
          meshKey: freshKey ? "fresh" : "auto",
        },
        onEvent
      );
      setOutcome(result);
      setAuthorizing((previous) => ({ ...previous, running: false, done: true }));
      // Re-authorizing rewrites the record — the identifier above is editable, and the
      // generation timestamp the dashboard's rail reads has just moved. Without this the
      // cached copy keeps describing the hub as it was until the app is remounted.
      await refresh();
    } catch (error) {
      setAuthorizing((previous) => ({
        ...previous,
        running: false,
        error: typeof error === "string" ? error : String(error),
      }));
    }
  }, [deployment, server, identifier, selected, meshOnly, freshKey, reachability, refresh]);

  if (registryLoading) return null;

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
        onCancel={stopWaiting}
      />
      <Page
        menu={
          <AppMenu
            back={`/dashboard/${deployment.id}`}
            breadcrumb={`${deployment.name} · authorize`}
          />
        }
        buttons={
          <>
            <Button
              disabled={
                (!meshOnly && selected.length === 0) ||
                busy ||
                !server ||
                identifier.trim() === ""
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

          {forgotten && !authorizing.done && (
            <Alert className="max-w-2xl">
              This hub was authorized before konstruktor started keeping a record of the
              addresses it handed over, so what it currently advertises is not known here.
              Everything reachable has been ticked — check it before authorizing, because
              what you send now replaces what the coordination server has.
            </Alert>
          )}

          {authorizing.done && (
            <Alert className="max-w-2xl border-primary/50">
              <div className="flex flex-col gap-1">
                <span>
                  This hub is authorized and its service configuration has been rewritten.
                  Restart the stack from the dashboard to pick it up.
                </span>
                {outcome?.mesh_requested && !outcome.mesh_granted && (
                  <span className="text-destructive">
                    A mesh key was asked for, but the coordination server did not grant one.
                  </span>
                )}
                {outcome?.mesh_granted && (
                  <strong>
                    A mesh key was granted. It is single-use and expires 15 minutes after it
                    was issued — restart the stack before then.
                  </strong>
                )}
                {outcome?.reporter_enabled && (
                  <span>The hub reports its own health to the coordination server while it runs.</span>
                )}
              </div>
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
            {onMesh ? (
              <label className="mt-4 flex items-start gap-3 cursor-pointer text-sm">
                <input
                  type="checkbox"
                  className="mt-1"
                  checked={freshKey}
                  onChange={(event) => setFreshKey(event.target.checked)}
                />
                <span>
                  <span className="font-medium">Get a fresh mesh key</span>
                  <span className="block text-muted-foreground">
                    For a hub whose key expired before it joined the mesh — keys are
                    single-use and live 15 minutes. A hub that already joined keeps its
                    identity without one.
                  </span>
                </span>
              </label>
            ) : (
              <p className="mt-4 text-sm text-muted-foreground">
                This hub is not on a mesh yet, so a mesh key is asked for with this
                authorization.
              </p>
            )}
          </div>

          {meshOnly ? (
            <Alert className="max-w-2xl">
              This hub is reached over the mesh only. It is advertised at its mesh name,
              and inside Docker for plugin apps running next to it — nothing on this
              machine's networks, so there are no addresses to pick.
            </Alert>
          ) : (
          <div>
            <div className="flex flex-row items-center justify-between max-w-2xl mb-3">
              <SectionHeading>Addresses</SectionHeading>
              {settings.proberEndpoint?.trim() && (
                <Button
                  variant="outline"
                  size="sm"
                  disabled={probing || selected.length === 0}
                  onClick={checkReachability}
                >
                  <RadioTower className="size-3.5" />
                  {probing ? "Checking…" : "Check from outside"}
                </Button>
              )}
            </div>
            <HostPicker
              candidates={candidates}
              presets={presets}
              selected={selected}
              reach={reach}
              onReachChange={(preset) => {
                setSelected(selectionFor(candidates, preset));
                setReach(preset.id);
              }}
              onToggle={toggle}
              reachability={reachability}
              // The stack may well be up here, so a probe can actually succeed.
              canProbe={Boolean(settings.proberEndpoint?.trim())}
              loading={loading}
              error={error}
            />
          </div>
          )}

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
