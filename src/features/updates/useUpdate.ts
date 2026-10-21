import { useEffect, useState } from "react";
import { commands, type UpdateInfo } from "../../api";
import { errorText } from "../../lib/errors";

/** Checks the update feed once at startup; `install` downloads, installs and restarts. */
export function useUpdate() {
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    commands.checkUpdate().then(setUpdate, () => setUpdate(null));
  }, []);

  const install = async () => {
    setInstalling(true);
    setError(null);
    try {
      await commands.installUpdate(); // restarts the app on success
    } catch (e) {
      setError(errorText(e));
      setInstalling(false);
    }
  };

  return { update, installing, error, install };
}

export type UpdateState = ReturnType<typeof useUpdate>;
