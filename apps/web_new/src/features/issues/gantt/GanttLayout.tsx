// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline layout entry (parity/specs/issues.md §6, ISS-044–059/116).
// The layout root renders this with the entity's display preferences: it
// orders the already-fetched rows (manual order by default), colors bars
// from the project's states, docks the shared peek panel for a clicked
// row, and owns the timeline's write seams — sidebar reorder in manual
// sort, bar move/resize, block planting and the sticky quick-add. Edit
// standing gates every one of them; the read-only chart stays whole.
// Bulk builds add sidebar selection with the shared bulk toolbar, and a
// sentinel trails the sidebar while the server holds more rows.

import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";

import { useCreateWorkItem } from "../api/create.js";
import {
  BulkToolbar,
  isBulkOperationsEnabled,
  selectedStateIds,
  useBulkMutations,
  useBulkSelection,
} from "../bulk/index.js";
import { WorkItemPreviewHover } from "../lifecycle/WorkItemPreviewCard.js";
import { todayIsoDay } from "../dropdowns/calendar.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import type { IssuesEntity } from "../prefs/entities.js";
import type { LayoutPeekProps, PeekSelection } from "../peek/usePeekAddress.js";
import { usePeekAddress } from "../peek/usePeekAddress.js";
import { deriveReorder, type DropPlacement } from "./drag.js";
import { GanttChart, type GanttSelectionBinding } from "./GanttChart.js";
import { GanttQuickAdd } from "./GanttQuickAdd.js";
import { canEditTimeline } from "./permissions.js";
import { buildRows, type GanttRow } from "./rows.js";
import { addDays } from "./scale.js";
import {
  stateColorOf,
  useGanttPeek,
  useGanttStates,
  useProjectArchived,
  useProjectBulkFlags,
  useProjectMemberRole,
} from "./useGanttData.js";
import { useGanttMutations } from "./useGanttMutations.js";

// The peek is the whole detail composition; it loads on first open.
const LazyIssuePeek = React.lazy(() =>
  import("../peek/ResolvingIssuePeek.js").then((module) => ({ default: module.ResolvingIssuePeek }))
);

/** The single selection bucket behind the sidebar's header tri-state. */
const TIMELINE_SELECTION_GROUP = "gantt";

/**
 * Orders that make the sidebar reorder meaningful (ISS-050). Only the
 * ascending manual rank enables drag: a descending `-sort_order` list
 * would insert a rank that the server's ascending sort cannot reproduce,
 * so the gesture is inert there.
 */
export function isManualOrder(orderBy: string | null | undefined): boolean {
  return orderBy === "sort_order";
}

export interface GanttLayoutProps extends LayoutPeekProps {
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
  /** True for a completed module: the quick-add is hidden (MOD-030). */
  completedModule?: boolean;
  /**
   * The issues-bearing entity whose list prefix holds these rows (ISS-002).
   * Cycle and module timelines pass theirs so a drop patches the cache it
   * reads; absent means the route project's own list.
   */
  entity?: IssuesEntity | undefined;
  /** Controlled peeked work-item id (the route owns the URL param). */
  /** Receives the selection the route should write, or null on close (ISS-167). */
  /** Peeked project from the URL, when it differs from the route (ISS-167). */
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

/**
 * The selection a timeline row opens (ISS-165/167): the row's own project,
 * taken from the list column the row was built from (a cross-project child
 * reads under its own project), or the route's when the list no longer
 * holds that row.
 */
export function ganttRowPeek(row: GanttRow, issues: readonly IssueListItem[], routeProjectId: string): PeekSelection {
  const held = issues.find((entry) => entry.id === row.id);
  return { issueId: row.id, projectId: held?.project_id ?? routeProjectId, nestingLevel: 0 };
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
  completedModule = false,
  entity,
  peekKey,
  onPeekChange,
  peekProjectId,
}: GanttLayoutProps): React.ReactElement {
  const rows = React.useMemo(() => orderedRows(issues, prefs, projectIdentifier), [issues, prefs, projectIdentifier]);
  const states = useGanttStates(workspaceSlug, projectId);
  const barColors = React.useMemo(() => barColorsFor(rows, states), [rows, states]);
  const peek = usePeekAddress({ projectId, peekKey, peekProjectId, onPeekChange });
  const openFromAddress = React.useCallback(
    (row: GanttRow) => {
      peek.open(ganttRowPeek(row, issues, projectId));
    },
    [issues, peek, projectId]
  );
  // Every activation goes through the address, so a route-owned URL and a
  // standalone timeline share one mount and one close.
  const { openRow } = useGanttPeek(workspaceSlug, openFromAddress);
  const projectRole = useProjectMemberRole(workspaceSlug, projectId);
  // An archived project hides every edit affordance (ISS-222). The
  // layout reads that from the list the route already holds.
  const archived = useProjectArchived(workspaceSlug, projectId);
  const mutations = useGanttMutations(workspaceSlug, projectId, entity ?? { scope: "project", projectId });
  const create = useCreateWorkItem(workspaceSlug);
  const canEdit = canEditTimeline({
    projectRole,
    archived,
  });
  // Bulk selection needs the cloud operations flag plus edit standing;
  // the community build renders no checkboxes at all (ISS-116).
  const bulkEnabled = isBulkOperationsEnabled() && canEdit;
  const selectionItems = React.useMemo(
    () =>
      rows
        .filter((row) => !row.unconfirmed && !row.isEpic)
        .map((row) => ({ id: row.id, groupKey: TIMELINE_SELECTION_GROUP, stateId: row.stateId })),
    [rows]
  );
  const selection = useBulkSelection(selectionItems, bulkEnabled);
  const bulk = useBulkMutations({ workspaceSlug, projectId });
  const bulkFlags = useProjectBulkFlags(workspaceSlug, projectId);
  // A selection can only exist while the checkboxes do; forcing the
  // count to zero when the binding drops (a mid-session demotion) keeps
  // the toolbar from lingering over a checkbox-free sidebar.
  const selectedCount = bulkEnabled ? selection.selectedIds.size : 0;
  const selectionBinding: GanttSelectionBinding | null = bulkEnabled
    ? {
        isSelected: selection.isSelected,
        onRowToggle: selection.onRowToggle,
        groupState: selection.groupState(TIMELINE_SELECTION_GROUP),
        onGroupToggle: () => selection.onGroupToggle(TIMELINE_SELECTION_GROUP),
        hasSelectables: selectionItems.length > 0,
      }
    : null;
  // Bars and sidebar links open the hover preview card (ISS-133); the
  // timeline's own state read already holds the name and group.
  const renderPreview = React.useCallback(
    (row: GanttRow, trigger: React.ReactElement<Record<string, unknown>>) => {
      const state = states.find((entry) => entry.id === row.stateId);
      return (
        <WorkItemPreviewHover
          trigger={trigger}
          issue={{
            id: row.id,
            name: row.name,
            sequence_id: row.sequenceId,
            priority: row.priority,
            start_date: row.startIso,
            target_date: row.targetIso,
            state_id: row.stateId,
          }}
          projectIdentifier={projectIdentifier}
          state={state === undefined ? undefined : { name: state.name, group: state.group }}
        />
      );
    },
    [projectIdentifier, states]
  );

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
      completedModule={completedModule}
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
        peekedId={peek.peeked}
        onOpenRow={openRow}
        hasMore={totalCount > rows.length}
        selection={selectionBinding}
        reorder={reorder}
        canEdit={canEdit}
        onDatesChange={(issueId, dates) => mutations.moveBar(issueId, dates)}
        onPlantBlock={(issueId, dates) => mutations.plantBlock(issueId, dates)}
        quickAdd={quickAdd}
        projectId={projectId}
        renderPreview={renderPreview}
      />
      {selectedCount === 0 ? null : (
        <BulkToolbar
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          count={selectedCount}
          fromStateIds={selectedStateIds(selectionItems, selection.selectedIds)}
          hideCycle={bulkFlags.hideCycle}
          hideModule={bulkFlags.hideModule}
          hideEstimate={bulkFlags.hideEstimate}
          weekStart={weekStart}
          busy={bulk.update.isPending || bulk.remove.isPending || bulk.archive.isPending || bulk.subscribe.isPending}
          onClear={selection.clear}
          onUpdate={(properties) => {
            void bulk.update
              .mutateAsync({ issueIds: [...selection.selectedIds], properties })
              .then(() => selection.clear())
              .catch(() => undefined);
          }}
          onDelete={() => {
            void bulk.remove
              .mutateAsync([...selection.selectedIds])
              .then(() => selection.clear())
              .catch(() => undefined);
          }}
          onArchive={() => {
            void bulk.archive
              .mutateAsync([...selection.selectedIds])
              .then(() => selection.clear())
              .catch(() => undefined);
          }}
          onSubscribe={() => {
            void bulk.subscribe
              .mutateAsync([...selection.selectedIds])
              .then(() => selection.clear())
              .catch(() => undefined);
          }}
        />
      )}
      {peek.peeked === null ? null : (
        <React.Suspense fallback={null}>
          <LazyIssuePeek
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            projectIdentifier={projectIdentifier}
            peeked={peek.peeked}
            peekProjectId={peek.peekProjectId}
            rows={issues}
            onPeekChange={peek.report}
            onClose={peek.close}
          />
        </React.Suspense>
      )}
    </div>
  );
}
