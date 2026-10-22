import { useCallback, useEffect, useState } from "react";
import { commands, type Script } from "../../api";

/** Saved scripts, most recently saved first. */
export function useScripts() {
  const [scripts, setScripts] = useState<Script[]>([]);
  /** The saved scripts have been read (or failed to): until then, an unknown id may just not be loaded yet. */
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    commands
      .listScripts()
      .then(setScripts, () => setScripts([]))
      .finally(() => setLoaded(true));
  }, []);

  /** Saves and returns the stored script (with its id); throws on failure. */
  const save = useCallback(async (script: Script) => {
    const saved = await commands.saveScript(script);
    setScripts((all) => [saved, ...all.filter((s) => s.id !== saved.id)]);
    return saved;
  }, []);

  const remove = useCallback(async (id: string) => {
    await commands.deleteScript(id);
    setScripts((all) => all.filter((s) => s.id !== id));
  }, []);

  return { scripts, loaded, save, remove };
}
