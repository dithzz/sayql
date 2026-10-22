import { useEffect } from "react";
import { commands, subscribeToHandoffs } from "../../api";
import { useLatest } from "../../hooks/useLatest";
import { applyHandoff, type HandoffActions } from "./applyHandoff";

/**
 * Shows handoffs from AI apps as they arrive, including any that came in before the window loaded.
 * Waits for `ready` (the saved scripts loaded): a link that launches the app arrives at once, and an
 * open-script handoff taken before then would find no such script. Handoffs wait in the backend.
 */
export function useHandoffs(actions: HandoffActions, ready = true) {
  const latest = useLatest(actions);
  useEffect(() => {
    if (!ready) return;
    const drain = () =>
      commands.takeHandoffs().then(
        (list) => list.forEach((h) => applyHandoff(h, latest.current)),
        () => {},
      );
    const unlisten = subscribeToHandoffs(drain);
    drain();
    return () => {
      unlisten.then((u) => u());
    };
  }, [latest, ready]);
}
