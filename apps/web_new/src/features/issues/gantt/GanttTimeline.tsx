// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline for issues lists that render outside the project timeline route
// (e.g. a saved view opened on the timeline layout). The chart math, header,
// sidebar and full-screen portal are a heavy leaf, so the route chunk only
// carries this lazy boundary and the skeleton fallback.

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
}

/** The timeline reads the profile's week start; the layout root owns prefs. */
export function GanttTimeline({
  workspaceSlug,
  projectId,
  projectIdentifier,
  issues,
  totalCount,
  prefs,
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
      />
    </React.Suspense>
  );
}
