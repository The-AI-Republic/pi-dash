// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline layout entry (parity/specs/issues.md §6, ISS-044–049). The
// layout root renders this with the entity's display preferences: it
// orders the already-fetched rows (manual order by default), colors bars
// from the project's states, docks the shared peek panel for a clicked
// row and hands the rest to the chart. Issue loading, pagination and the
// empty states stay in the layout root; drag, resize, add-block,
// quick-add, bar chrome and permissions belong to the sibling slices.

import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";

import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import { GanttChart } from "./GanttChart.js";
import { GanttPeekPanel } from "./GanttPeekPanel.js";
import { buildRows, type GanttRow } from "./rows.js";
import { stateColorOf, useGanttPeek, useGanttStates } from "./useGanttData.js";

export interface GanttLayoutProps {
  workspaceSlug: string;
  projectId: string;
  projectIdentifier: string;
  issues: readonly IssueListItem[];
  /** Server total across all pages; the header's live count (ISS-044). */
  totalCount: number;
  prefs: DisplayPrefs;
  /** Profile's first day of the week (ISS-045). */
  weekStart: number;
}

/** Map ordered rows to their state color, skipping uncolored states. */
export function barColorsFor(
  rows: readonly GanttRow[],
  states: Parameters<typeof stateColorOf>[0]
): Record<string, string> {
  const colors: Record<string, string> = {};
  for (const row of rows) {
    const color = stateColorOf(states, row.stateId);
    if (color !== null) colors[row.id] = color;
  }
  return colors;
}

/**
 * Rows in the preference's order. The list query sends the stored
 * `order_by` to the server, so the response already arrives in the order
 * the timeline should draw (manual rank for `sort_order`, newest/oldest
 * for the date sorts): building the rows must not re-sort or reverse them
 * (ISS-044/045, ISS-050).
 */
export function orderedRows(
  issues: readonly IssueListItem[],
  _prefs: Pick<DisplayPrefs, "orderBy">,
  projectIdentifier: string
): GanttRow[] {
  return buildRows(issues, projectIdentifier);
}

export function GanttLayout({
  workspaceSlug,
  projectId,
  projectIdentifier,
  issues,
  totalCount,
  prefs,
  weekStart,
}: GanttLayoutProps): React.ReactElement {
  const rows = React.useMemo(() => orderedRows(issues, prefs, projectIdentifier), [issues, prefs, projectIdentifier]);
  const states = useGanttStates(workspaceSlug, projectId);
  const barColors = React.useMemo(() => barColorsFor(rows, states), [rows, states]);
  const { peek, openRow, closePeek } = useGanttPeek(workspaceSlug);

  return (
    <div data-testid="gantt-layout" className="flex h-full min-h-0 flex-col">
      <GanttChart
        rows={rows}
        workspaceSlug={workspaceSlug}
        totalCount={totalCount}
        weekStart={weekStart}
        barColors={barColors}
        peekedId={peek?.row.id ?? null}
        onOpenRow={openRow}
      />
      {peek !== null ? (
        <GanttPeekPanel workspaceSlug={workspaceSlug} projectId={projectId} row={peek.row} onClose={closePeek} />
      ) : null}
    </div>
  );
}
