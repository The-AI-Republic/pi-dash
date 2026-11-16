// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issues calendar layout (parity/specs/issues.md §4, ISS-021–027). The
// visible range drives one windowed target-date-grouped read; the header
// persists month/week and weekend options, and day tiles own blocks,
// quick-add, drag drops, per-day paging and the compact day detail.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";

import { DeleteWorkItemDialog } from "../../../shared/shell/contextual/index.js";
import { getAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";
import type { RichExpression } from "../filters/rich/expression.js";
import { entityListsKeyPart } from "../prefs/entities.js";
import { CalendarReloadBar } from "../loaders/skeletons.js";
import { AddExistingModal } from "../list/AddExistingModal.js";
import { CALENDAR_ADD_EXISTING_ITEM, CALENDAR_ADD_ITEM, CALENDAR_TITLE_PLACEHOLDER } from "./CalendarDay.js";
import { selectDisplayPrefs, useEntityPrefs, useUpdateDisplayPrefs } from "../prefs/displayPrefs.js";
import type { IssuesEntity } from "../prefs/entities.js";
import { useWeekStart } from "../dropdowns/weekStart.js";
import { CalendarGrid } from "./CalendarGrid.js";
import { CalendarHeader } from "./CalendarHeader.js";
import {
  useCalendarAttach,
  useCalendarCreate,
  useCalendarDelete,
  useRedateIssue,
  type CalendarMembershipContext,
} from "./actions.js";
import { createTempId, isTempRowId } from "../freshness/fresh.js";
import { tempIssueRow } from "../freshness/tempRows.js";
import {
  decideDrop,
  groupRowsByDate,
  monthTitle,
  monthWeeks,
  rangeForCells,
  stepMonth,
  stepWeek,
  weekRow,
  weekTitle,
  weekdayLabels,
  type CalendarOptions,
} from "./model.js";
import {
  MONTH_PER_PAGE,
  WEEK_PER_PAGE,
  calendarDayQueryOptions,
  calendarQueryOptions,
  calendarStatesQueryOptions,
  dayTotalFromResponse,
  rowsFromResponse,
} from "./query.js";
import { useNarrowViewport, useTouchInput } from "./useMedia.js";
import { addPendingRow, removePendingRow, type PendingRowsByDay } from "./pending.js";
import { calendarDayAtPoint } from "./dropTarget.js";
import { PEEK_ISSUE_PARAM, type LayoutPeekProps, type PeekSelection } from "../peek/usePeekAddress.js";
import { usePeekAddress } from "../peek/usePeekAddress.js";

// The peek is the whole detail composition; it loads on first open.
const LazyIssuePeek = React.lazy(() =>
  import("../peek/ResolvingIssuePeek.js").then((module) => ({ default: module.ResolvingIssuePeek }))
);

export const PEEK_QUERY_KEY = PEEK_ISSUE_PARAM;

export interface CalendarLayoutProps extends LayoutPeekProps {
  workspaceSlug: string;
  projectId: string;
  projectIdentifier: string;
  entity: IssuesEntity;
  /** Members/admins on a live project may add, drag and quick-act. */
  canEdit: boolean;
  /** Epics-only calendars offer creation, not add-existing. */
  canAddExisting?: boolean;
  /** Cycle or module whose detail screen hosts this calendar. */
  membership?: CalendarMembershipContext;
  /**
   * Stored saved-view expression. A view opens its calendar with the same
   * conditions its list applies (VIEW-028); a bare entity calendar sends
   * none and reads every dated row.
   */
  richExpression?: RichExpression | undefined;
  /** Controlled peeked work-item id (the route owns the URL param). */
  /** Receives the selection the route should write, or null on close (ISS-167). */
  /** Peeked project from the URL, when it differs from the route (ISS-167). */
}

function isoToday(): string {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
}

function selectMode(value: unknown): CalendarOptions["mode"] {
  return value === "week" ? "week" : "month";
}

function rowsForDay(grouped: ReadonlyMap<string, readonly IssueListItem[]>, day: string): readonly IssueListItem[] {
  return grouped.get(day) ?? [];
}

export function CalendarLayout({
  workspaceSlug,
  projectId,
  projectIdentifier,
  entity,
  canEdit,
  canAddExisting = true,
  membership,
  richExpression = {},
  peekKey,
  onPeekChange,
  peekProjectId,
}: CalendarLayoutProps): React.ReactElement {
  const narrow = useNarrowViewport();
  const touch = useTouchInput();
  const weekStart = useWeekStart();
  const { data: record } = useEntityPrefs(workspaceSlug, entity);
  const prefs = record ? selectDisplayPrefs(record) : null;
  const { updateDisplayPrefs } = useUpdateDisplayPrefs(workspaceSlug, entity);
  const [anchor, setAnchor] = React.useState(() => new Date());
  const [selectedDay, setSelectedDay] = React.useState<string | null>(null);
  const [highlightedId, setHighlightedId] = React.useState<string | null>(null);
  const [dropTarget, setDropTarget] = React.useState<string | null>(null);
  const [dragged, setDragged] = React.useState<IssueListItem | null>(null);
  const dropTargetRef = React.useRef<string | null>(null);
  const [addExistingDay, setAddExistingDay] = React.useState<string | null>(null);
  const [extraByDay, setExtraByDay] = React.useState<Record<string, IssueListItem[]>>({});
  const [dayTotals, setDayTotals] = React.useState<Record<string, number>>({});
  const [dayCursors, setDayCursors] = React.useState<Record<string, string>>({});
  const peek = usePeekAddress({ projectId, peekKey, peekProjectId, onPeekChange });
  const [deleteTarget, setDeleteTarget] = React.useState<IssueListItem | null>(null);
  const [loadingDay, setLoadingDay] = React.useState<string | null>(null);
  const [reloadBar, setReloadBar] = React.useState(false);
  const [pendingByDay, setPendingByDay] = React.useState<PendingRowsByDay>({});

  const mode = selectMode(prefs?.calendar.mode);
  const showWeekends = prefs?.calendar.showWeekends ?? false;
  const year = anchor.getFullYear();
  const month0 = anchor.getMonth();
  const today = isoToday();

  const weeks = React.useMemo(
    () =>
      mode === "month" ? monthWeeks(year, month0, weekStart, showWeekends) : [weekRow(anchor, weekStart, showWeekends)],
    [mode, year, month0, anchor, weekStart, showWeekends]
  );
  const range = rangeForCells(weeks);
  const perPage = mode === "month" ? MONTH_PER_PAGE : WEEK_PER_PAGE;
  const calendarQuery = useQuery(
    calendarQueryOptions(getAppClient(), workspaceSlug, projectId, entity, { ...range, perPage }, richExpression)
  );
  const statesQuery = useQuery(calendarStatesQueryOptions(getAppClient(), workspaceSlug, projectId));
  const queryClient = useQueryClient();
  const redate = useRedateIssue(workspaceSlug, projectId, entity);
  const create = useCalendarCreate(workspaceSlug, projectId, membership, entity);
  const attach = useCalendarAttach(workspaceSlug, projectId, membership, entity);
  const remove = useCalendarDelete(workspaceSlug, projectId, entity);

  // Pointer capture on the block means the tile never sees the release;
  // resolve the destination from the document so a real drag still drops.
  React.useEffect(() => {
    if (dragged === null || typeof globalThis.document === "undefined") return;
    const doc = globalThis.document;
    const onMove = (event: PointerEvent): void => {
      const day = calendarDayAtPoint(doc, event.clientX, event.clientY);
      if (day === null) return;
      dropTargetRef.current = day;
      setDropTarget(day);
    };
    const onUp = (event: PointerEvent): void => {
      const day = calendarDayAtPoint(doc, event.clientX, event.clientY) ?? dropTargetRef.current;
      handleDrop(day);
    };
    const onCancel = (): void => {
      dropTargetRef.current = null;
      setDragged(null);
      setDropTarget(null);
    };
    doc.addEventListener("pointermove", onMove);
    doc.addEventListener("pointerup", onUp);
    doc.addEventListener("pointercancel", onCancel);
    return () => {
      doc.removeEventListener("pointermove", onMove);
      doc.removeEventListener("pointerup", onUp);
      doc.removeEventListener("pointercancel", onCancel);
    };
  });
  const rows = React.useMemo(
    () => (calendarQuery.data === undefined ? [] : rowsFromResponse(calendarQuery.data)),
    [calendarQuery.data]
  );
  const grouped = React.useMemo(() => groupRowsByDate(rows), [rows]);
  const totalsByDay = React.useMemo(() => {
    const totals = new Map<string, number>();
    if (calendarQuery.data !== undefined) {
      for (const [day, bucket] of Object.entries(calendarQuery.data.results)) totals.set(day, bucket.total_results);
    }
    for (const [day, total] of Object.entries(dayTotals)) totals.set(day, total);
    return totals;
  }, [calendarQuery.data, dayTotals]);
  const extraRowsByDay = React.useMemo(() => {
    const map = new Map<string, IssueListItem[]>();
    for (const [day, extra] of Object.entries(extraByDay)) map.set(day, extra);
    for (const [day, pending] of Object.entries(pendingByDay)) {
      const existing = map.get(day) ?? [];
      map.set(day, [...existing, ...pending.filter((row) => !existing.some((entry) => entry.id === row.id))]);
    }
    return map;
  }, [extraByDay, pendingByDay]);
  const stateColors = React.useMemo(() => {
    const colors = new Map<string, string>();
    for (const state of statesQuery.data ?? []) colors.set(state.id, state.color);
    return colors;
  }, [statesQuery.data]);
  const stateMeta = React.useMemo(() => {
    const meta = new Map<string, { name: string; group: string }>();
    for (const state of statesQuery.data ?? []) meta.set(state.id, { name: state.name, group: state.group });
    return meta;
  }, [statesQuery.data]);

  React.useEffect(() => {
    if (!calendarQuery.isFetching) {
      setReloadBar(false);
      return;
    }
    setReloadBar(true);
  }, [calendarQuery.isFetching]);

  const title = mode === "month" ? monthTitle(year, month0) : weekTitle(weeks[0] ?? []);

  function changeOptions(next: Partial<CalendarOptions>): void {
    if (prefs === null) return;
    updateDisplayPrefs({
      calendar: {
        mode: next.mode ?? prefs.calendar.mode,
        showWeekends: next.showWeekends ?? prefs.calendar.showWeekends,
      },
    });
  }

  function moveMonth(delta: number): void {
    const next = stepMonth(year, month0, delta);
    setAnchor(new Date(next.year, next.month0, 1, 12));
  }

  function moveWeek(delta: number): void {
    setAnchor((current) => stepWeek(current, delta));
  }

  function goToday(): void {
    setAnchor(new Date());
    setSelectedDay(today);
  }

  function pickMonth(nextMonth0: number, nextYear: number): void {
    setAnchor(new Date(nextYear, nextMonth0, 1, 12));
  }

  function loadMore(day: string): void {
    const cursor = dayCursors[day] ?? `${perPage}:1:0`;
    setLoadingDay(day);
    void queryClient
      .fetchQuery(
        calendarDayQueryOptions(
          getAppClient(),
          workspaceSlug,
          projectId,
          entity,
          { day, perPage, cursor },
          richExpression
        )
      )
      .then((response) => {
        const incoming = rowsFromResponse(response);
        setExtraByDay((current) => {
          const existing = current[day] ?? [];
          const merged = [...existing];
          for (const row of incoming) {
            if (!merged.some((entry) => entry.id === row.id)) merged.push(row);
          }
          return { ...current, [day]: merged };
        });
        setDayTotals((current) => ({ ...current, [day]: dayTotalFromResponse(response, day) }));
        setDayCursors((current) => ({
          ...current,
          [day]: response.next_page_results ? response.next_cursor : cursor,
        }));
      })
      .finally(() => setLoadingDay(null));
  }

  function handleDrop(day: string | null): void {
    if (dragged === null) {
      setDropTarget(null);
      return;
    }
    if (day === null) {
      dropTargetRef.current = null;
      setDragged(null);
      setDropTarget(null);
      return;
    }
    const decision = decideDrop(dragged, day);
    if (decision.kind === "apply") {
      redate.reDate({ issue: dragged, destination: day });
      setHighlightedId(dragged.id);
    } else if (decision.kind === "refuse") {
      redate.reDate({ issue: dragged, destination: day });
    }
    setDragged(null);
    dropTargetRef.current = null;
    setDropTarget(null);
  }

  function quickAdd(day: string, name: string): void {
    const tempId = createTempId();
    setPendingByDay((current) =>
      addPendingRow(current, day, { ...tempIssueRow(tempId, name), target_date: day, project_id: projectId })
    );
    create.mutate({ name, day }, { onSettled: () => setPendingByDay((current) => removePendingRow(current, tempId)) });
  }

  /** Open the peek on a block, naming it in the address (ISS-167). */
  function openPeek(issue: IssueListItem): void {
    const selection: PeekSelection = { issueId: issue.id, projectId: issue.project_id, nestingLevel: 0 };
    peek.open(selection);
  }

  return (
    <div className="relative flex h-full min-h-0 flex-col p-(--space-4)" data-testid="issues-calendar">
      {reloadBar ? <CalendarReloadBar /> : null}
      <CalendarHeader
        title={title}
        mode={mode}
        showWeekends={showWeekends}
        pickerYear={year}
        pickerMonth0={month0}
        onPrev={() => (mode === "month" ? moveMonth(-1) : moveWeek(-1))}
        onNext={() => (mode === "month" ? moveMonth(1) : moveWeek(1))}
        onToday={goToday}
        onPickMonth={pickMonth}
        onOptionsChange={changeOptions}
      />
      <CalendarGrid
        weeks={weeks}
        weekdayLabels={weekdayLabels(weekStart)}
        rowsByDay={grouped}
        totalsByDay={totalsByDay}
        extraRowsByDay={extraRowsByDay}
        loadingDay={loadingDay}
        workspaceSlug={workspaceSlug}
        projectIdentifier={projectIdentifier}
        stateColors={stateColors}
        stateMeta={stateMeta}
        highlightedId={highlightedId}
        narrow={narrow && touch}
        canEdit={canEdit}
        canDrag={canEdit && !touch}
        canAddExisting={canAddExisting}
        selectedDay={selectedDay}
        dropTarget={dropTarget}
        dragging={dragged !== null}
        onSelect={setSelectedDay}
        onDragOver={(day) => {
          if (dragged !== null) {
            dropTargetRef.current = day;
            setDropTarget(day);
          }
        }}
        onOpen={(issue) => {
          openPeek(issue);
        }}
        onEdit={(issue) => {
          if (typeof globalThis.window !== "undefined") {
            globalThis.window.location.href = `/${workspaceSlug}/browse/${projectIdentifier}-${issue.sequence_id}`;
          }
        }}
        onDelete={(issue) => setDeleteTarget(issue)}
        onDragStart={(issue, event) => {
          event.preventDefault();
          setDragged(issue);
        }}
        onQuickAdd={quickAdd}
        onAddExisting={setAddExistingDay}
        onLoadMore={loadMore}
      />
      {narrow && touch && selectedDay !== null ? (
        <MobileDayDetail
          day={selectedDay}
          rows={[...rowsForDay(grouped, selectedDay), ...(extraRowsByDay.get(selectedDay) ?? [])]}
          totalRows={totalsByDay.get(selectedDay) ?? 0}
          projectIdentifier={projectIdentifier}
          canEdit={canEdit}
          canAddExisting={canAddExisting}
          loadingMore={loadingDay === selectedDay}
          onAddExisting={setAddExistingDay}
          onQuickAdd={quickAdd}
          onLoadMore={loadMore}
        />
      ) : null}
      <AddExistingModal
        open={addExistingDay !== null}
        onOpenChange={(open) => {
          if (!open) setAddExistingDay(null);
        }}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        excludeIds={rows.map((row) => row.id)}
        undatedOnly
        attachableDay={addExistingDay ?? today}
        onAttach={async (issueIds) => {
          if (addExistingDay === null) return;
          attach.mutate(
            { issueIds, day: addExistingDay },
            {
              onSuccess: async () => {
                await queryClient.invalidateQueries({
                  queryKey: issueKeys.lists(workspaceSlug, ...entityListsKeyPart(entity)),
                });
                setAddExistingDay(null);
              },
            }
          );
        }}
      />
      {peek.peeked === null ? null : (
        <React.Suspense fallback={null}>
          <LazyIssuePeek
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            projectIdentifier={projectIdentifier}
            peeked={peek.peeked}
            peekProjectId={peek.peekProjectId}
            rows={rows}
            onPeekChange={peek.report}
            onClose={peek.close}
          />
        </React.Suspense>
      )}
      <DeleteWorkItemDialog
        open={deleteTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null);
        }}
        noun="Work item"
        title={deleteTarget?.name ?? ""}
        deleting={remove.isPending}
        onConfirm={() => {
          if (deleteTarget === null) return;
          remove.mutate(deleteTarget.id, { onSuccess: () => setDeleteTarget(null) });
        }}
      />
    </div>
  );
}

function MobileDayDetail({
  day,
  rows,
  totalRows,
  projectIdentifier,
  canEdit,
  canAddExisting,
  loadingMore,
  onAddExisting,
  onQuickAdd,
  onLoadMore,
}: {
  day: string;
  rows: readonly IssueListItem[];
  totalRows: number;
  projectIdentifier: string;
  canEdit: boolean;
  canAddExisting: boolean;
  loadingMore: boolean;
  onAddExisting: (day: string) => void;
  onQuickAdd: (day: string, title: string) => void;
  onLoadMore: (day: string) => void;
}): React.ReactElement {
  const [title, setTitle] = React.useState("");
  const [adding, setAdding] = React.useState(false);
  const hasMore = totalRows > rows.length;

  function submit(): void {
    const name = title.trim();
    if (name === "") return;
    onQuickAdd(day, name);
    setTitle("");
    setAdding(false);
  }

  return (
    <section
      aria-label={`Issues on ${day}`}
      className="mt-(--space-3) flex flex-col gap-(--space-2) border-t border-(--border) pt-(--space-3)"
    >
      {rows.length === 0 ? (
        <p className="text-body text-(--text-muted)">No work items on this day.</p>
      ) : (
        rows.map((issue) => {
          const temp = isTempRowId(issue.id);
          return (
            <div key={issue.id} className="relative flex items-center gap-(--space-2)">
              <span className="text-caption font-[family-name:var(--font-mono)] text-(--text-muted)">
                {temp ? "…" : `${projectIdentifier}-${issue.sequence_id}`}
              </span>
              <div className="text-body min-w-0 truncate text-(--text)">{issue.name}</div>
              {temp ? (
                <span
                  aria-hidden="true"
                  className="pointer-events-none absolute inset-0 animate-pulse bg-(--subtle)/60"
                />
              ) : null}
            </div>
          );
        })
      )}
      {hasMore ? (
        <button
          type="button"
          className="text-caption self-start text-(--text-muted) hover:text-(--text)"
          disabled={loadingMore}
          onClick={() => onLoadMore(day)}
        >
          Load more
        </button>
      ) : null}
      {canEdit ? (
        <div className="flex flex-col gap-(--space-1)">
          <button
            type="button"
            className="text-caption flex items-center gap-(--space-1) text-(--text-muted)"
            onClick={() => setAdding((open) => !open)}
          >
            {CALENDAR_ADD_ITEM}
          </button>
          {canAddExisting ? (
            <button
              type="button"
              className="text-caption flex items-center gap-(--space-1) text-(--text-muted)"
              onClick={() => onAddExisting(day)}
            >
              {CALENDAR_ADD_EXISTING_ITEM}
            </button>
          ) : null}
          {adding ? (
            <input
              autoFocus
              value={title}
              placeholder={CALENDAR_TITLE_PLACEHOLDER}
              aria-label="Work item title"
              className="text-caption w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-1) py-(--space-1) text-(--text) outline-none focus:border-(--accent)"
              onChange={(event) => setTitle(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") submit();
                if (event.key === "Escape") {
                  setAdding(false);
                  setTitle("");
                }
              }}
            />
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
