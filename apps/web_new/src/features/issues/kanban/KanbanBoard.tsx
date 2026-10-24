// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Kanban board layout (parity/specs/issues.md §5, ISS-028–043). Reads the
// entity's issues at the user's stored grouping and draws one fixed-width
// column per value under a sticky header row, or, with a sub-group set,
// the same columns once per lane beneath a sticky lane-header strip. In
// flat mode each column folds to a narrow strip through the stored kanban
// filters; in swimlane mode the lane headers fold instead. The show-empty
// flag hides zero-count columns and lanes, every column ends with its
// quick-add, and cards open the peek panel on a wide viewport and
// navigate to the work item on a narrow one. Unconfirmed creates render
// inert and cannot be opened. Column headers carry the add control, cards
// drag by pointer between seats, columns, lanes and the delete zone, and
// every column pages past its first window on its own.

import { useQuery, useSuspenseQuery } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";
import { Skeleton } from "@pidash/kit";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useMe, usePermissions } from "../../../core/session/index.js";
import { projectDetailsQueryOptions, projectsQueryOptions } from "../../projects/index.js";
import { useNarrowViewport } from "../../../shared/shell/narrowViewport.js";
import { notifyInfo } from "../../../shared/shell/index.js";
import { useCreateWorkItem } from "../api/create.js";
import { useTouchInput } from "../calendar/useMedia.js";
import type { DisplayFeatureFlags } from "../display/matrix.js";
import type { RichExpression } from "../filters/rich/expression.js";
import { isTempRowId } from "../freshness/fresh.js";
import { useAttachExisting } from "../list/attach.js";
import { isCollapsed, readCollapsedGroups, useToggleCollapsedGroup } from "../list/collapse.js";
import { creationDisabledForGroup } from "../list/grouping.js";
import { attachCreatedIssue } from "../list/mutations.js";
import { useListReferences } from "../list/reference.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import type { IssuesEntity } from "../prefs/entities.js";
import { prefsQueryOptions } from "../prefs/prefsQuery.js";
import { cardKey, KanbanCard } from "./KanbanCard.js";
import {
  ColumnSkeleton,
  KanbanColumn,
  KanbanColumnSentinel,
  KanbanLaneColumn,
  KanbanLoadMore,
} from "./KanbanColumn.js";
import { KanbanDeleteZone } from "./KanbanDeleteZone.js";
import { KanbanHeaderCreate } from "./KanbanHeaderCreate.js";
import { KanbanVirtualCard } from "./KanbanVirtualCard.js";
import {
  boardCreateInitialValues,
  boardPrefill,
  boardQuickAddDisabledReason,
  effectiveSubGroup,
  type BoardLane,
} from "./model.js";
import { boardCellOf, useBoard } from "./pages.js";
import { KanbanLaneHeader } from "./KanbanLaneHeader.js";
import { copyCardLink, KanbanPeek } from "./KanbanPeek.js";
import { KanbanQuickAdd } from "./KanbanQuickAdd.js";
import { useBoardCardMove } from "./mutations.js";
import { useBoardDrag, type DraggedCard } from "./useBoardDrag.js";
import type { BoardDropPoint, CardDropEdge } from "./dropTarget.js";
import { boardDropRefusal, insertionIndexForDrop, MOVE_REFUSED_TITLE, planBoardMove } from "./drag.js";
import { creationBlockedInColumn, dropBlockedInColumn } from "../edition/workflow.js";

const LazyCreateDialog = React.lazy(() =>
  import("../components/WorkItemCreateDialog.js").then((module) => ({ default: module.WorkItemCreateDialog }))
);
const LazyAddExistingModal = React.lazy(() =>
  import("../list/AddExistingModal.js").then((module) => ({ default: module.AddExistingModal }))
);
const LazyDeleteDialog = React.lazy(() =>
  import("../lifecycle/DeleteWorkItemDialog.js").then((module) => ({ default: module.DeleteWorkItemDialog }))
);

/** How long the dropped card keeps its highlight. */
export const DROPPED_HIGHLIGHT_MS = 2000;

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
  /**
   * Edition workflow reason blocking one drop column, given the state the
   * dragged card is leaving; OSS never supplies one, and with the gate off
   * the seam below answers nothing.
   */
  dropBlockedReason?: ((columnKey: string, fromStateId: string | null) => string | undefined) | undefined;
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
  dropBlockedReason,
}: KanbanBoardProps): React.ReactElement {
  const client = getAppClient();
  const navigate = useNavigate();
  const narrow = useNarrowViewport();
  const touch = useTouchInput();
  const me = useMe();
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
  const move = useBoardCardMove(workspaceSlug, projectId, entity);
  const attach = useAttachExisting({ workspaceSlug, projectId, entity, cycleId, moduleId });
  const [localPeekKey, setLocalPeekKey] = React.useState<string | null>(null);
  const peekKey = controlledPeekKey === undefined ? localPeekKey : controlledPeekKey;
  const [createTarget, setCreateTarget] = React.useState<{ columnKey: string; laneKey: string | null } | null>(null);
  const [attachOpen, setAttachOpen] = React.useState(false);
  const [deleteIssue, setDeleteIssue] = React.useState<IssueListItem | null>(null);
  const [droppedId, setDroppedId] = React.useState<string | null>(null);
  const boardRef = React.useRef<HTMLDivElement | null>(null);
  const drag = useBoardDrag(boardRef, commitDrop);

  const editAllowed = permissions.canEdit && (canEdit ?? true);
  const readOnly = !editAllowed || completedCycle;
  const cardDraggable = !readOnly && !touch;
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

  React.useEffect(() => {
    if (droppedId === null) return;
    const timer = globalThis.setTimeout(() => setDroppedId(null), DROPPED_HIGHLIGHT_MS);
    return () => globalThis.clearTimeout(timer);
  }, [droppedId]);

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
  const hoverColumn = drag.target?.columnKey ?? null;
  const hoverLane = drag.target?.laneKey ?? null;

  function keyOf(issue: Pick<IssueListItem, "sequence_id">): string {
    return cardKey(projectIdentifier, issue);
  }

  /**
   * Edition workflow reason for one column. The host may supply its own
   * seam; otherwise the workflow gate answers, and in the community build
   * it answers nothing for every column.
   */
  function workflowReasonFor(columnKey: string, fromStateId: string | null): string | undefined {
    if (dropBlockedReason !== undefined) return dropBlockedReason(columnKey, fromStateId);
    return dropBlockedInColumn({ projectId, groupDimension: prefs.groupBy, columnKey, fromStateId });
  }

  /** Edition workflow reason creation is closed in one column. */
  function creationReasonFor(columnKey: string): string | undefined {
    return creationBlockedReason ?? creationBlockedInColumn({ projectId, groupDimension: prefs.groupBy, columnKey });
  }

  /**
   * One released drag: the delete zone hands the card to the delete
   * confirmation, a drop back onto its own seat stays silent, a refused
   * drop explains itself — even a reorder inside its own cell — and
   * every other drop writes the destination value plus its new sort
   * position and highlights the landed card.
   */
  function commitDrop(dragged: DraggedCard, target: BoardDropPoint | null): void {
    if (target === null) return;
    if (target.kind === "delete") {
      setDeleteIssue(dragged.issue);
      return;
    }
    const sameCell = target.columnKey === dragged.groupKey && (target.laneKey ?? "") === (dragged.laneKey ?? "");
    const destCell = boardCellOf(board, target.columnKey, target.laneKey);
    if (destCell === null) return;
    const index = insertionIndexForDrop(
      destCell.rows,
      dragged.issue.id,
      dragged.sourceIndex,
      target.issueId,
      target.edge
    );
    if (sameCell && index === dragged.sourceIndex) return;
    const refusal = boardDropRefusal({
      orderBy: prefs.orderBy,
      groupDimension: prefs.groupBy,
      subDimension: subGroup,
      canEdit: editAllowed,
      destColumnKey: target.columnKey,
      openCycleIds: refs.openCycleIds,
      ...(workflowReasonFor(target.columnKey, dragged.issue.state_id) === undefined
        ? {}
        : { workflowBlockedReason: workflowReasonFor(target.columnKey, dragged.issue.state_id) }),
    });
    if (refusal !== null) {
      notifyInfo(MOVE_REFUSED_TITLE, refusal.toast);
      return;
    }
    const plan = planBoardMove({
      issue: dragged.issue,
      groupDimension: prefs.groupBy,
      subDimension: subGroup,
      sourceColumn: dragged.groupKey,
      sourceLane: dragged.laneKey,
      destColumn: target.columnKey,
      destLane: target.laneKey,
      destRows: destCell.rows.filter((row) => row.id !== dragged.issue.id),
      index,
      sourceIndex: dragged.sourceIndex,
    });
    if (plan === null) return;
    move.moveCard({ issueId: dragged.issue.id, plan, currentModuleIds: dragged.issue.module_ids });
    setDroppedId(dragged.issue.id);
  }

  /** Drop-disabled notice over the hovered column or cell, if any. */
  function overlayForCell(columnKey: string, laneKey: string | null): string | null {
    if (drag.dragged === null || hoverColumn === null || hoverColumn !== columnKey) return null;
    if ((hoverLane ?? "") !== (laneKey ?? "")) return null;
    const refusal = boardDropRefusal({
      orderBy: prefs.orderBy,
      groupDimension: prefs.groupBy,
      subDimension: subGroup,
      canEdit: editAllowed,
      destColumnKey: columnKey,
      openCycleIds: refs.openCycleIds,
      ...(workflowReasonFor(columnKey, drag.dragged.issue.state_id) === undefined
        ? {}
        : { workflowBlockedReason: workflowReasonFor(columnKey, drag.dragged.issue.state_id) }),
    });
    return refusal?.overlay ?? null;
  }

  /** Insertion edge at one card of the hovered seat, if any. */
  function indicatorFor(columnKey: string, laneKey: string | null, issueId: string): CardDropEdge | null {
    if (drag.target === null || drag.target.kind !== "card") return null;
    if (drag.target.columnKey !== columnKey || (drag.target.laneKey ?? "") !== (laneKey ?? "")) return null;
    if (drag.target.issueId !== issueId) return null;
    return drag.target.edge;
  }

  /** Whether the hovered column or cell takes the card at its end. */
  function endIndicatorFor(columnKey: string, laneKey: string | null): boolean {
    if (drag.target === null || drag.target.kind === "card" || drag.target.kind === "delete") return false;
    if (drag.target.columnKey !== columnKey) return false;
    return (drag.target.laneKey ?? "") === (laneKey ?? "");
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
    if (!showQuickAdd || creationReasonFor(columnKey) !== undefined) return null;
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

  function headerCreate(columnKey: string, columnTitle: string, laneKey: string | null): React.ReactNode {
    const disabledReason = readOnly
      ? "New work items are read-only for you"
      : creationDisabledForGroup(prefs.groupBy)
        ? "Creation is off in this grouping"
        : creationReasonFor(columnKey);
    return (
      <KanbanHeaderCreate
        columnTitle={columnTitle}
        menu={cycleId !== undefined || moduleId !== undefined}
        disabled={disabledReason !== undefined}
        {...(disabledReason === undefined ? {} : { disabledReason })}
        onCreate={() => setCreateTarget({ columnKey, laneKey })}
        onAddExisting={() => setAttachOpen(true)}
      />
    );
  }

  function columnAfterRows(columnKey: string): React.ReactNode {
    if (read.columnFetching(columnKey)) return <ColumnSkeleton />;
    if (!read.columnHasMore(columnKey)) return null;
    return <KanbanColumnSentinel onVisible={() => read.loadMoreColumn(columnKey)} />;
  }

  function cellAfterRows(columnKey: string, laneKey: string): React.ReactNode {
    if (read.cellFetching(columnKey, laneKey)) return <ColumnSkeleton />;
    if (!read.cellHasMore(columnKey, laneKey)) return null;
    return <KanbanLoadMore onLoad={() => read.loadMoreCell(columnKey, laneKey)} />;
  }

  function dropIndicator(): React.ReactNode {
    return (
      <div
        key="drop-indicator"
        data-testid="kanban-drop-indicator"
        aria-hidden="true"
        className="h-0.5 shrink-0 rounded-full bg-(--accent)"
      />
    );
  }

  function cellRows(rows: IssueListItem[], columnKey: string, laneKey: string | null): React.ReactNode[] {
    const overlay = overlayForCell(columnKey, laneKey);
    const out: React.ReactNode[] = [];
    rows.forEach((issue, index) => {
      if (overlay === null && indicatorFor(columnKey, laneKey, issue.id) === "above") out.push(dropIndicator());
      out.push(renderCard(issue, columnKey, laneKey, index));
      if (overlay === null && indicatorFor(columnKey, laneKey, issue.id) === "below") out.push(dropIndicator());
    });
    if (overlay === null && endIndicatorFor(columnKey, laneKey)) out.push(dropIndicator());
    return out;
  }

  function renderCard(
    issue: IssueListItem,
    groupKey: string,
    laneKey: string | null,
    index: number
  ): React.ReactElement {
    const dragging = drag.dragged?.issue.id === issue.id;
    const dropped = droppedId !== null && droppedId === issue.id && !dragging;
    return (
      <KanbanVirtualCard
        key={`${issue.id}-${groupKey}-${laneKey ?? ""}`}
        issueId={issue.id}
        groupKey={groupKey}
        laneKey={laneKey}
        forceContent={dragging || dropped}
      >
        <KanbanCard
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
          draggable={cardDraggable}
          dragging={dragging}
          dropped={dropped}
          onDragStart={(row) => {
            setDroppedId(null);
            drag.startDrag({ issue: row, groupKey, laneKey, sourceIndex: index });
          }}
        />
      </KanbanVirtualCard>
    );
  }

  function closeCreate(): void {
    setCreateTarget(null);
  }

  async function attachExisting(ids: string[]): Promise<void> {
    const done = await attach.attach(ids);
    if (done) setAttachOpen(false);
  }

  const createPrefill =
    createTarget === null ? null : boardPrefill(prefs.groupBy, createTarget.columnKey, subGroup, createTarget.laneKey);
  const createInitialValues =
    createPrefill === null
      ? null
      : {
          ...boardCreateInitialValues(createPrefill),
          ...(cycleId === undefined ? {} : { cycleId }),
          ...(moduleId === undefined ? {} : { moduleIds: [moduleId] }),
        };

  const flatLane = lanes[0];

  return (
    <div
      data-testid="kanban-layout"
      data-sub-grouped={board.subGrouped ? "true" : undefined}
      className="relative flex h-full min-h-0"
    >
      <KanbanDeleteZone active={drag.dragged !== null} hovered={drag.target?.kind === "delete"} />
      <div
        data-testid="kanban-board"
        ref={boardRef}
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
                  <span className="ml-auto shrink-0">{headerCreate(column.key, column.title, null)}</span>
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
                      rows={cellRows(lane.cells[column.key]?.rows ?? [], column.key, lane.key)}
                      footer={quickAdd(column.key, lane.key)}
                      dropOverlayText={overlayForCell(column.key, lane.key)}
                      afterRows={cellAfterRows(column.key, lane.key)}
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
                rows={cellRows(flatLane?.cells[column.key]?.rows ?? [], column.key, null)}
                loading={read.isFetching}
                collapsed={isCollapsed(collapsedState, "group_by", column.key)}
                onToggleCollapse={() => collapse.toggleCollapsedGroup("group_by", column.key)}
                footer={quickAdd(column.key, null)}
                create={headerCreate(column.key, column.title, null)}
                dropOverlayText={overlayForCell(column.key, null)}
                afterRows={columnAfterRows(column.key)}
              />
            ))}
          </div>
        )}
      </div>
      {createTarget === null || createInitialValues === null ? null : (
        <React.Suspense fallback={null}>
          <LazyCreateDialog
            open
            onOpenChange={(open) => {
              if (!open) closeCreate();
            }}
            workspaceSlug={workspaceSlug}
            projects={
              project === null
                ? []
                : [
                    {
                      id: project.id,
                      name: project.name,
                      identifier: project.identifier,
                      memberRole: project.member_role,
                      archived: project.archived_at !== null,
                    },
                  ]
            }
            initialProjectId={projectId}
            projectIdentifier={projectIdentifier}
            initialValues={createInitialValues}
            onCreated={closeCreate}
          />
        </React.Suspense>
      )}
      {attachOpen ? (
        <React.Suspense fallback={null}>
          <LazyAddExistingModal
            open
            onOpenChange={(open) => {
              if (!open) setAttachOpen(false);
            }}
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            excludeIds={boardRows(lanes).map((row) => row.id)}
            excludeCycleMembers={cycleId !== undefined}
            {...(moduleId !== undefined ? { excludeModuleId: moduleId } : {})}
            onAttach={(ids) => attachExisting(ids)}
          />
        </React.Suspense>
      ) : null}
      {deleteIssue === null ? null : (
        <React.Suspense fallback={null}>
          <LazyDeleteDialog
            open
            onOpenChange={(open) => {
              if (!open) setDeleteIssue(null);
            }}
            scope={{ workspaceSlug, projectId, issueId: deleteIssue.id }}
            projectIdentifier={projectIdentifier}
            sequenceId={deleteIssue.sequence_id}
            viewerId={me.data?.id ?? null}
            creatorId={deleteIssue.created_by}
            projectRole={project?.member_role ?? null}
          />
        </React.Suspense>
      )}
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
