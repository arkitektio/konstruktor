import { useEffect, useState } from "react";

import * as api from "../../api";
import type { DeploymentRecord } from "../../api";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";

/** Radix refuses an empty value, so "no hub" needs a name of its own. */
const NONE = "__none__";

/** The hubs registered on this machine — the ones an engine can join the network of. */
export const useRegisteredHubs = () => {
  const [hubs, setHubs] = useState<DeploymentRecord[]>([]);
  useEffect(() => {
    api
      .listDeployments()
      .then((all) => setHubs(all.filter((d) => d.kind === "hub")))
      .catch((e) => console.error("Could not list the hubs", e));
  }, []);
  return hubs;
};

/** Picks a hub's folder, or none. */
export const HubPicker = ({
  hubs,
  value,
  onChange,
  disabled,
}: {
  hubs: DeploymentRecord[];
  value: string | null;
  onChange: (hubPath: string | null) => void;
  disabled?: boolean;
}) => (
  <Select
    value={value ?? NONE}
    onValueChange={(v) => onChange(v === NONE ? null : v)}
    disabled={disabled}
  >
    <SelectTrigger className="w-full">
      <SelectValue />
    </SelectTrigger>
    <SelectContent>
      <SelectItem value={NONE}>No hub — plugins run on the engine's own network</SelectItem>
      {hubs.map((hub) => (
        <SelectItem key={hub.id} value={hub.path}>
          {hub.name}
        </SelectItem>
      ))}
    </SelectContent>
  </Select>
);
