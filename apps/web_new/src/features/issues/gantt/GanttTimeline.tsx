// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline for issues lists that render outside the project timeline route
// (e.g. a saved view opened on the timeline layout). The chart math, header,
// sidebar and full-screen portal are a heavy leaf, so the route chunk only
// carries this lazy boundary and the skeleton fallback. The boundary itself
// stays out of the eager route chain: both issue routes request it through
// the lazy leaves module, and the skeleton shows while its chunk lands.

import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";

import { ListSkeleton } from "../loaders/ListSkeleton.js";
import { useWeekStart } from "../dropdowns/weekStart.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";

const LazyGanttLayout = React.lazy(() =>
  import("./GanttLayout.js").then((module) => ({ default: module.GanttLayout }))
);

export interface GanttTimelineProps {
  workspaceSlug: string;
  projectId: string;
  projectIdentifier: string;
  issues: readonly IssueListItem[];
  totalCount: number;
  prefs: DisplayPrefs;
  /**
   * True when the timeline belongs to a completed cycle; the quick-add
   * stays hidden (ISS-055). Project and saved-view mounts leave it false,
   * and the cycle route passes the cycle's status when it lands.
   */
  completedCycle?: boolean;
  /**
   * True when the timeline belongs to a completed module; the quick-add
   * stays hidden like a completed cycle's (MOD-030, NEWFRONT-802).
   */
  completedModule?: boolean;
}

/** The timeline reads the profile's week start; the layout root owns prefs. */
export function GanttTimeline({
  workspaceSlug,
  projectId,
  projectIdentifier,
  issues,
  totalCount,
  prefs,
  completedCycle = false,
  completedModule = false,
}: GanttTimelineProps): React.ReactElement {
  const weekStart = useWeekStart();
  return (
    <React.Suspense fallback={<ListSkeleton />}>
      <LazyGanttLayout
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectIdentifier={projectIdentifier}
        issues={issues}
        totalCount={totalCount}
        prefs={prefs}
        weekStart={weekStart}
        completedCycle={completedCycle}
        completedModule={completedModule}
      />
    </React.Suspense>
  );
}
