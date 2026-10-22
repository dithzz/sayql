import { useEffect } from "react";
import { commands, subscribeToHandoffs } from "../../api";
import { useLatest } from "../../hooks/useLatest";
import { applyHandoff, type HandoffActions } from "./applyHandoff";

/** Shows handoffs from AI apps as they arrive, including any that came in before the window loaded. */
export function useHandoffs(actions: HandoffActions) {
  const latest = useLatest(actions);
  useEffect(() => {
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
  }, [latest]);
}
