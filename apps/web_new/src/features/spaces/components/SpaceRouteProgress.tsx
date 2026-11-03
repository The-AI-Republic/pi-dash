// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Route progress (parity/specs/space.md SPC-017). Slow transitions in
// the subtree — the legacy resolution is the proof — hold an explicit
// lease while in flight; the bar appears only once a transition has
// been pending past the show delay, so instant transitions never
// flash one, and it is removed from the document the moment the last
// lease settles. Leases are explicit rather than router-driven because
// the subtree fetches in components (no route loaders), so router
// pending state would miss the very transitions that need the bar.
// Gate DOM contract: div[role="progressbar"]
// data-testid="space-route-progress", present only while a slow
// transition is in flight.
import * as React from "react";

import { spaceMessage } from "../lib/spaceMessages.js";

interface SpaceProgressValue {
  active: boolean;
  acquire: () => () => void;
}

const SpaceProgressContext = React.createContext<SpaceProgressValue | null>(null);

/** How long a transition pends before the bar appears. */
export const SPACE_PROGRESS_DELAY_MS = 200;

export function SpaceProgressProvider({ children }: { children: React.ReactNode }): React.ReactElement {
  const [leases, setLeases] = React.useState(0);
  const acquire = React.useCallback(() => {
    setLeases((count) => count + 1);
    return () => {
      setLeases((count) => count - 1);
    };
  }, []);
  const value = React.useMemo(() => ({ active: leases > 0, acquire }), [leases, acquire]);
  return <SpaceProgressContext.Provider value={value}>{children}</SpaceProgressContext.Provider>;
}

/**
 * Hold a progress lease while `active`. Components with a slow
 * transition (the legacy resolver) report through this; outside the
 * provider it is a no-op, so out-of-tree renders stay quiet.
 */
export function useSpaceProgress(active: boolean): void {
  const progress = React.useContext(SpaceProgressContext);
  React.useEffect(() => {
    if (!active || progress === null) return;
    return progress.acquire();
  }, [active, progress]);
}

export function SpaceRouteProgress({
  delayMs = SPACE_PROGRESS_DELAY_MS,
}: {
  delayMs?: number | undefined;
}): React.ReactElement | null {
  const progress = React.useContext(SpaceProgressContext);
  const [shown, setShown] = React.useState(false);
  React.useEffect(() => {
    if (progress === null || !progress.active) {
      setShown(false);
      return;
    }
    const timer = setTimeout(() => setShown(true), delayMs);
    return () => clearTimeout(timer);
  }, [progress, delayMs]);
  if (!shown) return null;
  return (
    <div
      role="progressbar"
      aria-label={spaceMessage("progress.loading")}
      data-testid="space-route-progress"
      className="fixed inset-x-0 top-0 z-50 h-0.5 bg-transparent"
    >
      <div className="h-full w-full bg-(--accent) motion-safe:animate-pulse" />
    </div>
  );
}
