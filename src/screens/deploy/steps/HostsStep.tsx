import { Globe } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useFormContext } from "react-hook-form";
import { ErrorDisplay } from "../../../components/Error";
import { HostPicker } from "../HostPicker";
import * as api from "../../../api";
import { StepFrame } from "../../wizard/StepFrame";

/**
 * The addresses the coordination server will hand out for this hub's services.
 *
 * These are decided here, before the manifest is sent, because the manifest carries
 * them: an alias added afterwards means going through the authorization again.
 */
export const HostsStep = () => {
  const { setValue, getValues } = useFormContext();
  const [candidates, setCandidates] = useState<api.HostCandidate[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Read once, on mount: after the effect below runs the form value mirrors the
  // selection, and feeding it back in would pin the picker to its first state.
  const previous = useRef<string[]>(
    ((getValues("hosts") ?? []) as { host: string }[]).map((h) => h.host)
  );
  const [selected, setSelected] = useState<Set<string>>(new Set(previous.current));

  useEffect(() => {
    api
      .hostCandidates()
      .then((found) => {
        setCandidates(found);
        // Nothing chosen yet: pre-tick what the machine recommends. Resolved names are
        // offered but never assumed — they only work if the client's DNS agrees.
        if (previous.current.length === 0) {
          setSelected(new Set(found.filter((c) => c.recommended).map((c) => c.value)));
        }
      })
      .catch((e) => setError(typeof e === "string" ? e : String(e)))
      .finally(() => setLoading(false));
  }, []);

  const toggle = (value: string) =>
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(value)) next.delete(value);
      else next.add(value);
      return next;
    });

  useEffect(() => {
    const hosts = candidates
      .filter((c) => selected.has(c.value))
      .map((c) => ({ host: c.value, kind: c.kind }));
    setValue("hosts", hosts, { shouldValidate: true });
  }, [candidates, selected, setValue]);

  return (
    <StepFrame
      icon={Globe}
      title="Addresses"
      subtitle="Where can this hub be reached?"
      lead="Clients ask the coordination server where a service lives and get these addresses back, so pick the ones other machines on your network actually use. Loopback and virtual interfaces are left out — they only work from here."
    >
      <div className="flex flex-col gap-3">
        <HostPicker
          candidates={candidates}
          selected={selected}
          onToggle={toggle}
          loading={loading}
        />
        {error && <div className="text-sm text-destructive max-w-2xl">{error}</div>}
        <ErrorDisplay name="hosts" />
      </div>
    </StepFrame>
  );
};
