// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar UI state (parity/specs/cycles.md §14, CYC-035, CYC-039). The
// collapse flag survives reloads in the same browser; a fresh narrow
// viewport starts collapsed so the panel opens over the content instead
// of covering it, while an explicit stored choice always wins. The
// breakdown tab is remembered per cycle. Storage is injectable so tests
// never touch the browser profile.

import * as React from "react";

import { useNarrowViewport } from "../../../shared/shell/narrowViewport.js";
import {
  readBreakdownTab,
  readSidebarCollapsed,
  writeBreakdownTab,
  writeSidebarCollapsed,
  type CycleBreakdownTab,
  type SidebarFlagStorage,
} from "../lib/cycleSidebar.js";

/** Browser profile storage behind the sidebar flags. */
function browserStorage(): SidebarFlagStorage {
  return globalThis.localStorage;
}

/**
 * Whether the sidebar is collapsed, persisted across visits. The narrow
 * default applies only when no choice is stored yet; later viewport
 * changes never flip the panel under the user.
 */
export function useCycleSidebarCollapsed(
  storage: SidebarFlagStorage = browserStorage()
): [boolean, (next: boolean) => void] {
  const narrow = useNarrowViewport();
  const [collapsed, setCollapsedState] = React.useState(() => readSidebarCollapsed(storage) || narrow);
  const setCollapsed = React.useCallback(
    (next: boolean) => {
      setCollapsedState(next);
      writeSidebarCollapsed(storage, next);
    },
    [storage]
  );
  return [collapsed, setCollapsed];
}

/** One cycle's breakdown tab, remembered across visits. */
export function useCycleAnalyticsTab(
  cycleId: string,
  storage: SidebarFlagStorage = browserStorage()
): [CycleBreakdownTab, (next: CycleBreakdownTab) => void] {
  const [tab, setTabState] = React.useState(() => readBreakdownTab(storage, cycleId));
  const setTab = React.useCallback(
    (next: CycleBreakdownTab) => {
      setTabState(next);
      writeBreakdownTab(storage, cycleId, next);
    },
    [cycleId, storage]
  );
  return [tab, setTab];
}
