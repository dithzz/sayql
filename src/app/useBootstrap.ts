import { useEffect, useState } from "react";
import { commands, type Bootstrap } from "../api";
import { errorText } from "../lib/errors";

/** Loads saved connections and settings once at startup. */
export function useBootstrap() {
  const [data, setData] = useState<Bootstrap | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    commands.init().then(setData, (e) => setError(errorText(e)));
  }, []);
  return { data, error };
}
