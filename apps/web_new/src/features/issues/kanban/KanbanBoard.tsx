// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Kanban board layout (parity/specs/issues.md §5, ISS-028–035). Reads the
// entity's issues at the user's stored grouping and draws one fixed-width
// column per value under a sticky header row, or, with a sub-group set,
// the same columns once per lane beneath a sticky lane-header strip. In
// flat mode each column folds to a narrow strip through the stored kanban
// filters; in swimlane mode the lane headers fold instead. The show-empty
// flag hides zero-count columns and lanes, every column ends with its
// quick-add, and cards open the peek panel on a wide viewport and
// navigate to the work item on a narrow one. Unconfirmed creates render
// inert and cannot be opened.

import { useQuery, useSuspenseQuery } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";
import { Skeleton } from "@pidash/kit";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { usePermissions } from "../../../core/session/index.js";
import { projectDetailsQueryOptions, projectsQueryOptions } from "../../projects/index.js";
import { useNarrowViewport } from "../../../shared/shell/narrowViewport.js";
import { useCreateWorkItem } from "../api/create.js";
import type { DisplayFeatureFlags } from "../display/matrix.js";
import type { RichExpression } from "../filters/rich/expression.js";
import { isTempRowId } from "../freshness/fresh.js";
import { isCollapsed, readCollapsedGroups, useToggleCollapsedGroup } from "../list/collapse.js";
import { attachCreatedIssue } from "../list/mutations.js";
import { useListReferences } from "../list/reference.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import type { IssuesEntity } from "../prefs/entities.js";
import { prefsQueryOptions } from "../prefs/prefsQuery.js";
import { cardKey, KanbanCard } from "./KanbanCard.js";
import { KanbanColumn, KanbanLaneColumn } from "./KanbanColumn.js";
import { boardPrefill, boardQuickAddDisabledReason, effectiveSubGroup, type BoardLane } from "./model.js";
import { useBoard } from "./pages.js";
import { KanbanLaneHeader } from "./KanbanLaneHeader.js";
import { copyCardLink, KanbanPeek } from "./KanbanPeek.js";
import { KanbanQuickAdd } from "./KanbanQuickAdd.js";

export interface KanbanBoardProps {
  workspaceSlug: string;
  entity: IssuesEntity;
  projectId: string;
  projectIdentifier: string;
  prefs: DisplayPrefs;
  /** Project-aware edit standing from the host route. */
  canEdit?: boolean | undefined;
  /** True for a completed cycle: the board renders read-only. */
  completedCycle?: boolean | undefined;
  /** Stored rich filters scope every column read. */
  richFilters?: RichExpression | undefined;
  /** Controlled peek key (the route owns the URL param). */
  peekKey?: string | null | undefined;
  onPeekChange?: ((key: string | null) => void) | undefined;
  /** Cycle/module creation target for quick-add. */
  cycleId?: string | undefined;
  moduleId?: string | undefined;
  /** Edition workflow rule reason; OSS never supplies one. */
  creationBlockedReason?: string | undefined;
}

/** The browser path a card links to. */
export function cardHref(workspaceSlug: string, projectIdentifier: string, sequenceId: number): string {
  return `/${workspaceSlug}/browse/${projectIdentifier}-${sequenceId}`;
}

/** Lanes the board draws: zero-count lanes drop when show-empty is off. */
export function visibleLanes(lanes: readonly BoardLane[], showEmpty: boolean): BoardLane[] {
  return lanes.filter((lane) => showEmpty || lane.total > 0);
}

/** Every board row, in reading order, across lanes and columns. */
export function boardRows(lanes: readonly BoardLane[]): IssueListItem[] {
  const seen = new Set<string>();
  const rows: IssueListItem[] = [];
  for (const lane of lanes) {
    for (const cell of Object.values(lane.cells)) {
      for (const row of cell.rows) {
        if (seen.has(row.id)) continue;
        seen.add(row.id);
        rows.push(row);
      }
    }
  }
  return rows;
}

function BoardLoading(): React.ReactElement {
  return (
    <div data-testid="kanban-loading" className="flex h-full gap-(--space-3) p-(--space-3)">
      {Array.from({ length: 3 }, (_, index) => (
        <div key={index} className="flex w-[350px] shrink-0 flex-col gap-(--space-2)">
          <Skeleton height="20px" />
          <Skeleton height="72px" />
          <Skeleton height="72px" />
        </div>
      ))}
    </div>
  );
}

export function KanbanBoard({
  workspaceSlug,
  entity,
  projectId,
  projectIdentifier,
  prefs,
  canEdit,
  completedCycle = false,
  richFilters = {},
  peekKey: controlledPeekKey,
  onPeekChange,
  cycleId,
  moduleId,
  creationBlockedReason,
}: KanbanBoardProps): React.ReactElement {
  const client = getAppClient();
  const navigate = useNavigate();
  const narrow = useNarrowViewport();
  const permissions = usePermissions(workspaceSlug);
  const subGroup = effectiveSubGroup(prefs.groupBy, prefs.subGroupBy);
  const refs = useListReferences({ workspaceSlug, projectId, dimension: prefs.groupBy });
  const groupLabels = refs.labels[prefs.groupBy ?? ""] ?? new Map<string, string>();
  const subLabels =
    subGroup === null ? new Map<string, string>() : (refs.labels[subGroup] ?? new Map<string, string>());
  const read = useBoard(client, workspaceSlug, {
    entity,
    groupBy: prefs.groupBy,
    subGroupBy: prefs.subGroupBy,
    orderBy: prefs.orderBy,
    showSubIssues: prefs.showSubIssues,
    showEmpty: prefs.showEmptyGroups,
    richFilters,
    groupLabels,
    subLabels,
  });
  const { data: projects } = useQuery(projectsQueryOptions(client, workspaceSlug));
  const { data: details } = useQuery(projectDetailsQueryOptions(client, workspaceSlug));
  const project = projects?.find((row) => row.id === projectId) ?? null;
  const projectDetails = details?.find((row) => row.id === projectId) ?? null;
  const { data: prefsRecord } = useSuspenseQuery(prefsQueryOptions(client, workspaceSlug, entity));
  const collapsedState = readCollapsedGroups(prefsRecord.filters);
  const collapse = useToggleCollapsedGroup(workspaceSlug, entity);
  const create = useCreateWorkItem(workspaceSlug);
  const [localPeekKey, setLocalPeekKey] = React.useState<string | null>(null);
  const peekKey = controlledPeekKey === undefined ? localPeekKey : controlledPeekKey;

  const editAllowed = permissions.canEdit && (canEdit ?? true);
  const readOnly = !editAllowed || completedCycle;
  const flags: DisplayFeatureFlags = {
    hideCycle: !project?.cycle_view,
    hideModule: !project?.module_view,
    hideEstimate: projectDetails === null || projectDetails.estimate === null,
  };

  const setPeek = React.useCallback(
    (key: string | null) => {
      if (onPeekChange !== undefined) onPeekChange(key);
      else setLocalPeekKey(key);
    },
    [onPeekChange]
  );

  if (read.isPending || read.board === null) return <BoardLoading />;
  const board = read.board;
  const lanes = visibleLanes(board.lanes, prefs.showEmptyGroups);
  const quickAddReason = boardQuickAddDisabledReason({
    canEdit: editAllowed,
    completedCycle,
    ...(creationBlockedReason === undefined ? {} : { workflowBlockedReason: creationBlockedReason }),
  });
  const showQuickAdd = quickAddReason === undefined;
  const peekIssue = peekKey === null ? null : (boardRows(lanes).find((row) => keyOf(row) === peekKey) ?? null);

  function keyOf(issue: Pick<IssueListItem, "sequence_id">): string {
    return cardKey(projectIdentifier, issue);
  }

  function open(issue: IssueListItem): void {
    // Cards never hand an unconfirmed row here; the card itself stays inert.
    if (narrow) {
      void navigate({ to: "/$ws/browse/$issueKey", params: { ws: workspaceSlug, issueKey: keyOf(issue) } });
      return;
    }
    setPeek(keyOf(issue));
  }

  function quickAdd(columnKey: string, laneKey: string | null): React.ReactNode {
    if (!showQuickAdd) return null;
    const prefill = boardPrefill(prefs.groupBy, columnKey, subGroup, laneKey);
    const groupedCycle = typeof prefill["cycle_id"] === "string" ? prefill["cycle_id"] : undefined;
    const groupedModules = Array.isArray(prefill["module_ids"]) ? prefill["module_ids"] : undefined;
    const rest = { ...prefill };
    delete rest["cycle_id"];
    delete rest["module_ids"];
    const fields = {
      ...rest,
      ...(rest["state_id"] === undefined && projectDetails?.default_state != null
        ? { state_id: projectDetails.default_state }
        : {}),
    };
    const targetCycle = cycleId ?? groupedCycle;
    const targetModule = moduleId ?? groupedModules?.[0];
    return (
      <KanbanQuickAdd
        projectIdentifier={projectIdentifier}
        disabled={false}
        onCreate={async (title) => {
          const created = await create.mutateAsync({
            projectId,
            payload: { ...fields, name: title },
            projectIdentifier,
          });
          if (targetCycle !== undefined || targetModule !== undefined) {
            await attachCreatedIssue(client, workspaceSlug, projectId, created.id, {
              ...(targetCycle === undefined ? {} : { cycleId: targetCycle }),
              ...(targetModule === undefined ? {} : { moduleId: targetModule }),
            }).catch(() => {
              // The detail slice reports membership failures; the item
              // itself already landed in its column.
            });
          }
        }}
      />
    );
  }

  function renderCard(issue: IssueListItem, groupKey: string, laneKey: string | null): React.ReactElement {
    return (
      <KanbanCard
        key={`${issue.id}-${groupKey}-${laneKey ?? ""}`}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectIdentifier={projectIdentifier}
        issue={issue}
        groupKey={groupKey}
        laneKey={laneKey}
        properties={prefs.properties}
        flags={flags}
        readOnly={readOnly}
        peeked={peekKey === keyOf(issue)}
        estimateEnabled={projectDetails?.estimate != null}
        states={refs.stateRows}
        stateNames={refs.stateNames}
        assigneeNames={refs.assigneeNames}
        labelNames={refs.labelNames}
        cycleNames={refs.cycleNames}
        moduleNames={refs.moduleNames}
        openCycleIds={refs.openCycleIds}
        timeZone={projectDetails?.timezone ?? null}
        href={cardHref(workspaceSlug, projectIdentifier, issue.sequence_id)}
        onOpen={open}
        onCopyLink={(row) => void copyCardLink(workspaceSlug, keyOf(row))}
      />
    );
  }

  const flatLane = lanes[0];

  return (
    <div
      data-testid="kanban-layout"
      data-sub-grouped={board.subGrouped ? "true" : undefined}
      className="relative flex h-full min-h-0"
    >
      <div
        data-testid="kanban-board"
        className={board.subGrouped ? "h-full min-h-0 overflow-auto" : "h-full overflow-x-auto"}
      >
        {board.subGrouped ? (
          <div className="flex h-full min-w-max flex-col">
            <div className="sticky top-0 z-20 flex bg-(--bg)">
              <div className="w-[220px] shrink-0 border-r border-b border-(--border)" />
              {board.columns.map((column) => (
                <div
                  key={column.key}
                  data-testid="kanban-group-header"
                  data-column-key={column.key}
                  className="flex h-11 w-[350px] shrink-0 items-center gap-(--space-2) border-r border-b border-(--border) px-(--space-3)"
                >
                  <span
                    data-testid="kanban-group-name"
                    className="text-emphasis min-w-0 truncate text-(--text)"
                    title={column.title}
                  >
                    {column.title}
                  </span>
                  <span data-testid="kanban-group-count" className="text-caption shrink-0 text-(--text-muted)">
                    {column.total}
                  </span>
                </div>
              ))}
            </div>
            {lanes.map((lane) => {
              const laneCollapsed = isCollapsed(collapsedState, "sub_group_by", lane.key);
              return (
                <div key={lane.key} data-testid="kanban-lane" data-lane-key={lane.key} className="flex">
                  <div className="sticky left-0 z-10 w-[220px] shrink-0 bg-(--bg)">
                    <KanbanLaneHeader
                      name={lane.title}
                      count={lane.total}
                      collapsed={laneCollapsed}
                      onToggle={() => collapse.toggleCollapsedGroup("sub_group_by", lane.key)}
                    />
                  </div>
                  {board.columns.map((column) => (
                    <KanbanLaneColumn
                      key={`${lane.key}-${column.key}`}
                      laneKey={lane.key}
                      columnKey={column.key}
                      collapsed={laneCollapsed}
                      rows={(lane.cells[column.key]?.rows ?? []).map((issue) =>
                        renderCard(issue, column.key, lane.key)
                      )}
                      footer={quickAdd(column.key, lane.key)}
                    />
                  ))}
                </div>
              );
            })}
          </div>
        ) : (
          <div className="flex h-full min-w-max">
            {board.columns.map((column) => (
              <KanbanColumn
                key={column.key}
                columnKey={column.key}
                title={column.title}
                count={column.total}
                rows={(flatLane?.cells[column.key]?.rows ?? []).map((issue) => renderCard(issue, column.key, null))}
                loading={read.isFetching}
                collapsed={isCollapsed(collapsedState, "group_by", column.key)}
                onToggleCollapse={() => collapse.toggleCollapsedGroup("group_by", column.key)}
                footer={quickAdd(column.key, null)}
              />
            ))}
          </div>
        )}
      </div>
      {peekIssue === null ? null : (
        <KanbanPeek
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          projectIdentifier={projectIdentifier}
          issue={peekIssue}
          canRename={editAllowed && !isTempRowId(peekIssue.id)}
          onClose={() => setPeek(null)}
        />
      )}
    </div>
  );
}
