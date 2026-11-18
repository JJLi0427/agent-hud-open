// The HUD panel's Refresh gesture: read every account again now (the host's
// own gesture, not a poll) and let the panel's sections ask again with it.
import { useCallback, useState } from "react";

export interface PanelRefresh {
  /** Bumped after a refresh returns, so a section re-asks even where its own
   * data's revision did not move. */
  refreshed: number;
  refresh: () => Promise<void>;
}

/** `run` reads everything again; the panel shows a failure in its own footer,
 * so this one does not raise an app-wide toast inside a HUD window. */
export function usePanelRefresh(run: () => Promise<unknown>): PanelRefresh {
  const [refreshed, setRefreshed] = useState(0);
  const refresh = useCallback(async () => {
    await run();
    setRefreshed((count) => count + 1);
  }, [run]);
  return { refreshed, refresh };
}
