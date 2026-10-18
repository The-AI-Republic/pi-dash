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
 * Rows in the preference's order. The list arrives already sorted by the
 * server (manual order by default); only a reverse `-sort_order` needs a
 * local pass, and every other order is left as the server sent it.
 */
export function orderedRows(
  issues: readonly IssueListItem[],
  prefs: Pick<DisplayPrefs, "orderBy">,
  projectIdentifier: string
): GanttRow[] {
  const rows = buildRows(issues, projectIdentifier);
  if (prefs.orderBy !== "-sort_order") return rows;
  return [...rows].reverse();
}

export function GanttLayout({
  workspaceSlug,
  projectId,
  projectIdentifier,
  issues,
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
