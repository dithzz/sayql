import { useEffect } from "react";
import { commands, type ConnectionConfig, type TeamAccount } from "../../api";
import { useLatest } from "../../hooks/useLatest";
import { errorText } from "../../lib/errors";

/**
 * Loads the team's shared connections whenever someone signs in, and clears them on sign-out.
 * Signed out, it asks nothing of anyone: no team server is contacted.
 */
export function useTeamConnections(account: TeamAccount | null, apply: (list: ConnectionConfig[]) => void, onError: (message: string) => void) {
  const latest = useLatest({ apply, onError });
  const key = account ? `${account.server} ${account.email}` : null;
  useEffect(() => {
    if (!key) return latest.current.apply([]);
    let live = true;
    commands.teamConnections().then(
      (list) => live && latest.current.apply(list),
      (e) => live && latest.current.onError(errorText(e)),
    );
    return () => {
      live = false;
    };
  }, [key, latest]);
}
