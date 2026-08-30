import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { useRegistry } from "../registry/registry-context";

/** Emitted from `src-tauri/src/tray.rs` when a deployment is picked from the tray menu. */
const OPEN_EVENT = "tray:open-deployment";

/**
 * Follows the tray: picking a hub or engine there shows the window and lands on its
 * dashboard. The registry is refreshed first, since the tray may know about a deployment
 * created since this window last looked (the CLI writes the same registry).
 */
export const useTrayNavigation = () => {
  const navigate = useNavigate();
  const { refresh } = useRegistry();

  useEffect(() => {
    const unlisten = listen<string>(OPEN_EVENT, (event) => {
      void refresh();
      navigate(`/dashboard/${event.payload}`);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [navigate, refresh]);
};
