import { Globe } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useFormContext } from "react-hook-form";
import { ErrorDisplay } from "../../../components/Error";
import { HostPicker, ReachChoice, Reachability } from "../HostPicker";
import { defaultPreset, reachFor, selectionFor, toggleHost } from "../reach";
import * as api from "../../../api";
import { useSettings } from "../../../settings/settings-context";
import { StepFrame } from "../../wizard/StepFrame";

/**
 * The addresses the coordination server will hand out for this hub's services.
 *
 * These are decided here, before the manifest is sent, because the manifest carries
 * them: an alias added afterwards means going through the authorization again.
 *
 * Nothing is running yet at this point in the wizard — the stack is started from the
 * dashboard — so the only reachability question that can be answered here is what address
 * the internet sees this machine as. Whether a port is actually open has to wait, and the
 * picker says so rather than showing a cross.
 */
export const HostsStep = () => {
  const { setValue, getValues } = useFormContext();
  const { settings } = useSettings();

  const [candidates, setCandidates] = useState<api.HostCandidate[]>([]);
  const [presets, setPresets] = useState<api.ReachPreset[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [reachability, setReachability] = useState<Record<string, Reachability>>({});

  // Read once, on mount: after the effect below runs the form value mirrors the
  // selection, and feeding it back in would pin the picker to its first state.
  const previous = useRef<api.AdvertisedHost[]>(
    (getValues("hosts") ?? []) as api.AdvertisedHost[]
  );
  const [selected, setSelected] = useState<api.AdvertisedHost[]>(previous.current);
  const [reach, setReach] = useState<ReachChoice>("custom");

  useEffect(() => {
    // Which tailnet is this hub's? Only the coordination server knows, and only if it
    // says. Without an answer every tailnet address on this machine — the personal
    // tailscale most laptops already run — is listed as somebody else's, which before the
    // hub has joined anything is simply true.
    const server = (getValues("coordServer") ?? "").trim();
    const domain = server ? api.meshDomain(server).catch(() => null) : Promise.resolve(null);

    domain
      .then((domain) => api.hostCandidates({ domain }))
      .then(({ candidates, presets }) => {
        setCandidates(candidates);
        setPresets(presets);

        // Nothing chosen yet: open on the preset most people want. Something chosen
        // already — a step revisited — is kept, and only labelled.
        if (previous.current.length === 0) {
          const preset = defaultPreset(presets);
          if (preset) {
            setSelected(selectionFor(candidates, preset));
            setReach(preset.id);
          }
        } else {
          setReach(reachFor(presets, previous.current));
        }
      })
      .catch((e) => setError(typeof e === "string" ? e : String(e)))
      .finally(() => setLoading(false));
  }, []);

  // What the internet sees this machine as, if the user has configured somewhere to ask.
  // Failure is silence: this decorates the step, it does not gate it.
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

  useEffect(() => {
    setValue("hosts", selected, { shouldValidate: true });
  }, [selected, setValue]);

  return (
    <StepFrame
      icon={Globe}
      title="Addresses"
      subtitle="Where can this hub be reached?"
      lead="Clients ask the coordination server where a service lives and get these addresses back, so pick the ones other machines actually use. Choose how far the hub should reach, or tick addresses yourself — the ones a peer could never use are tucked away at the bottom, with the reason why."
    >
      <div className="flex flex-col gap-3">
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
          // Nothing is listening until the dashboard starts the stack, so a probe here
          // could only ever come back refused.
          canProbe={false}
          loading={loading}
          error={error}
        />
        <ErrorDisplay name="hosts" />
      </div>
    </StepFrame>
  );
};
