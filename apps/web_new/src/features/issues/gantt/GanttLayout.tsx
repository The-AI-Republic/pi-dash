// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline layout entry (parity/specs/issues.md §6, ISS-044–055). The
// layout root renders this with the entity's display preferences: it
// orders the already-fetched rows (manual order by default), colors bars
// from the project's states, docks the shared peek panel for a clicked
// row, and owns the timeline's write seams — sidebar reorder in manual
// sort, bar move/resize, block planting and the sticky quick-add. Edit
// standing gates every one of them; the read-only chart stays whole.

import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";

import { usePermissions } from "../../../core/session/index.js";
import { useCreateWorkItem } from "../api/create.js";
import { todayIsoDay } from "../dropdowns/calendar.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import { deriveReorder, type DropPlacement } from "./drag.js";
import { GanttChart } from "./GanttChart.js";
import { GanttPeekPanel } from "./GanttPeekPanel.js";
import { GanttQuickAdd } from "./GanttQuickAdd.js";
import { canEditTimeline } from "./permissions.js";
import { buildRows, type GanttRow } from "./rows.js";
import { addDays } from "./scale.js";
import { stateColorOf, useGanttPeek, useGanttStates, useProjectMemberRole } from "./useGanttData.js";
import { useGanttMutations } from "./useGanttMutations.js";

/** Orders that make the sidebar reorder meaningful (ISS-050). */
export function isManualOrder(orderBy: string | null | undefined): boolean {
  return orderBy === "sort_order" || orderBy === "-sort_order";
}

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
  /** True for a completed cycle: the quick-add is hidden (ISS-055). */
  completedCycle?: boolean;
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
  completedCycle = false,
}: GanttLayoutProps): React.ReactElement {
  const rows = React.useMemo(() => orderedRows(issues, prefs, projectIdentifier), [issues, prefs, projectIdentifier]);
  const states = useGanttStates(workspaceSlug, projectId);
  const barColors = React.useMemo(() => barColorsFor(rows, states), [rows, states]);
  const { peek, openRow, closePeek } = useGanttPeek(workspaceSlug);
  const permissions = usePermissions(workspaceSlug);
  const projectRole = useProjectMemberRole(workspaceSlug, projectId);
  const mutations = useGanttMutations(workspaceSlug, projectId);
  const create = useCreateWorkItem(workspaceSlug);
  const canEdit = canEditTimeline({
    workspaceRole: permissions.role,
    projectRole,
    archived: false,
  });

  const reorder = React.useMemo(
    () =>
      canEdit && isManualOrder(prefs.orderBy)
        ? {
            onReorder: (sourceId: string, targetId: string, placement: DropPlacement) => {
              const result = deriveReorder(
                rows.map((row) => ({ id: row.id, sortOrder: row.sortOrder })),
                sourceId,
                targetId,
                placement
              );
              if (result !== null) mutations.reorder(sourceId, result.sortOrder, result.order);
            },
          }
        : null,
    [canEdit, mutations, prefs.orderBy, rows]
  );

  const today = todayIsoDay();
  const quickAdd = canEdit ? (
    <GanttQuickAdd
      enabled={canEdit}
      completedCycle={completedCycle}
      onCreate={(title) =>
        create.mutateAsync({
          projectId,
          payload: { name: title, start_date: today, target_date: addDays(today, 1) },
        })
      }
    />
  ) : undefined;

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
        reorder={reorder}
        canEdit={canEdit}
        onDatesChange={(issueId, dates) => mutations.moveBar(issueId, dates)}
        onPlantBlock={(issueId, dates) => mutations.plantBlock(issueId, dates)}
        quickAdd={quickAdd}
      />
      {peek !== null ? (
        <GanttPeekPanel workspaceSlug={workspaceSlug} projectId={projectId} row={peek.row} onClose={closePeek} />
      ) : null}
    </div>
  );
}
