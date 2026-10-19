// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Spreadsheet layout (parity/specs/issues.md §3, ISS-015–020). A flat,
// virtually rendered table over the cursor-paged issue list: sticky
// work-item column and header, display-driven property columns, inline
// picker edits that write immediately, header sort menus, a sticky
// quick-add, infinite pagination, sub-issue expansion (three levels,
// deeper opens peek), and arrow-key cell navigation.

import { Button, Menu, Spinner } from "@pidash/kit";
import { useInfiniteQuery, useQueries } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { ArrowDownWideNarrow, ArrowUpNarrowWide, ChevronRight, Eraser, MoreHorizontal, Plus } from "lucide-react";
import * as React from "react";
import type { IssueListItem } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { CopyWorkItemLinkButton } from "../../../shared/shell/CopyWorkItemLinkButton.js";
import { PeekPanel } from "../../../shared/shell/index.js";
import { useUpdateDisplayPrefs, type DisplayPrefs } from "../prefs/displayPrefs.js";
import type { IssuesEntity } from "../prefs/entities.js";
import { usePickerCycles } from "../../../shared/pickers/reference.js";
import type { RichExpression } from "../filters/rich/expression.js";
import { sheetSubIssuesQueryOptions } from "./subIssues.js";
import {
  isSheetRowReadOnly,
  moveSheetFocus,
  readStoredSheetSort,
  resolveSheetSort,
  SHEET_DEFAULT_ORDER,
  SHEET_SORT_ASCENDING_LABEL,
  SHEET_SORT_CLEAR_LABEL,
  SHEET_SORT_DESCENDING_LABEL,
  sheetFirstCellId,
  sheetSortOrder,
  visibleSheetColumns,
  writeStoredSheetSort,
  type SheetColumn,
  type SheetSortState,
  type SheetSortDirection,
} from "./model.js";
import {
  flattenSheetPages,
  sheetIssuesQueryOptions,
  useSheetCreateIssue,
  useSheetCycleUpdate,
  useSheetFieldUpdate,
  useSheetModuleUpdate,
} from "./data.js";
import { SheetPropertyCell, isEditableColumn, type SheetCellContext } from "./cells.js";
import { SpreadsheetSkeleton } from "../loaders/skeletons.js";

const ROW_HEIGHT = 44;

export interface SpreadsheetLayoutProps {
  workspaceSlug: string;
  projectId: string;
  projectIdentifier: string;
  entity: IssuesEntity;
  prefs: DisplayPrefs;
  canEdit: boolean;
  hideCycle: boolean;
  hideModule: boolean;
  /** The project has no estimate system, so the column stays hidden. */
  hideEstimate: boolean;
  /** Explicitly highlighted row id (drops); fresh rows highlight themselves. */
  highlightedId?: string | null;
  /** Stored rich filters shared with the list layout. */
  richExpression?: RichExpression;
  /** The profile's first day of week; the route owns the session read. */
  weekStart: number;
}

export interface DisplayRow {
  issue: IssueListItem;
  depth: number;
}

export function SpreadsheetLayout(props: SpreadsheetLayoutProps): React.ReactElement {
  const {
    workspaceSlug,
    projectId,
    projectIdentifier,
    entity,
    prefs,
    canEdit,
    hideCycle,
    hideModule,
    hideEstimate,
    highlightedId = null,
    richExpression = {},
    weekStart,
  } = props;
  const client = getAppClient();
  const query = useInfiniteQuery(
    sheetIssuesQueryOptions(client, workspaceSlug, projectId, {
      orderBy: prefs.orderBy,
      showSubIssues: prefs.showSubIssues,
      richExpression,
    })
  );
  const [expanded, setExpanded] = React.useState<ReadonlySet<string>>(new Set());
  const [peekIssue, setPeekIssue] = React.useState<IssueListItem | null>(null);
  const [scrolled, setScrolled] = React.useState(false);
  const [scrollNode, setScrollNode] = React.useState<HTMLDivElement | null>(null);
  const [localSort, setLocalSort] = React.useState<SheetSortState | null>(readStoredSheetSort);
  const cycles = usePickerCycles(workspaceSlug, projectId, true);
  const fields = useSheetFieldUpdate(workspaceSlug, projectId);
  const cycleWrite = useSheetCycleUpdate(workspaceSlug, projectId);
  const moduleWrite = useSheetModuleUpdate(workspaceSlug, projectId);
  const create = useSheetCreateIssue(workspaceSlug, projectId);
  const navigate = useNavigate();
  const { updateDisplayPrefs } = useUpdateDisplayPrefs(workspaceSlug, entity);

  const columns = React.useMemo(
    () => visibleSheetColumns(prefs.properties, { hideCycle, hideModule, hideEstimate }),
    [prefs.properties, hideCycle, hideModule, hideEstimate]
  );
  const activeSort = React.useMemo(
    () => resolveSheetSort(columns, prefs.orderBy, localSort),
    [columns, prefs.orderBy, localSort]
  );
  const expandedIds = React.useMemo(() => [...expanded], [expanded]);
  const childrenQueries = useQueries({
    queries: expandedIds.map((issueId) => sheetSubIssuesQueryOptions(client, workspaceSlug, projectId, issueId)),
  });
  const childrenByParent = React.useMemo(() => {
    const map = new Map<string, IssueListItem[]>();
    expandedIds.forEach((issueId, index) => {
      const rows = childrenQueries[index]?.data?.sub_issues ?? [];
      map.set(issueId, rows);
    });
    return map;
  }, [expandedIds, childrenQueries]);

  const roots = flattenSheetPages(query.data);
  const rows = React.useMemo(
    () => buildDisplayRows(roots, expanded, childrenByParent),
    [roots, expanded, childrenByParent]
  );
  const viewport = useViewportWindow(scrollNode, ROW_HEIGHT, rows.length);
  const visibleRows = rows.slice(viewport.start, viewport.end);
  const pendingFocus = React.useRef<{ row: number; column: number } | null>(null);

  React.useEffect(() => {
    if (viewport.atBottom && query.hasNextPage && !query.isFetchingNextPage) void query.fetchNextPage();
  }, [viewport.atBottom, query.hasNextPage, query.isFetchingNextPage, query.fetchNextPage]);

  const openSubIssues = (issue: IssueListItem): void => {
    void navigate({
      to: "/$ws/browse/$issueKey",
      params: { ws: workspaceSlug, issueKey: `${projectIdentifier}-${issue.sequence_id}` },
      hash: "sub-issues",
    });
  };
  const openWorkItem = (issue: IssueListItem): void => {
    void navigate({
      to: "/$ws/browse/$issueKey",
      params: { ws: workspaceSlug, issueKey: `${projectIdentifier}-${issue.sequence_id}` },
    });
  };
  const focusCell = (rowId: string, _columnKey: string, columnIndex: number): void => {
    const row = document.getElementById(sheetFirstCellId(rowId))?.closest("tr");
    const node = row?.querySelector<HTMLElement>(`td[data-column-index="${columnIndex}"]`);
    node?.focus();
  };

  const context: SheetCellContext = {
    workspaceSlug,
    projectId,
    editable: canEdit,
    weekStart,
    isRowReadOnly: (issue) => isSheetRowReadOnly(issue, cycles.data ?? []),
    onUpdate: (issueId, patch) => fields.mutateAsync({ issueId, patch }),
    onCycle: (issue, cycleId) =>
      cycleWrite.mutateAsync({ issueId: issue.id, cycleId, previousCycleId: issue.cycle_id }),
    onModules: (issue, moduleIds) =>
      moduleWrite.mutateAsync({ issueId: issue.id, moduleIds, previousModuleIds: issue.module_ids }),
    onOpenSubIssues: openSubIssues,
    focusCell,
  };

  const onGridKeyDown = (event: React.KeyboardEvent<HTMLTableCellElement>): void => {
    if (event.target !== event.currentTarget) return;
    const direction = arrowDirection(event.key);
    if (direction === null) return;
    event.preventDefault();
    const rowIndex = Number(event.currentTarget.dataset["rowIndex"]);
    const columnIndex = Number(event.currentTarget.dataset["columnIndex"]);
    const next = moveSheetFocus({ row: rowIndex, column: columnIndex }, direction, rows.length, columns.length + 1);
    if (focusSheetCell(document, rows, columns, next)) return;
    // The body is windowed, so a destination past the rendered slice has
    // no DOM node yet. Reveal it, then focus once the window recomputes.
    pendingFocus.current = { row: next.row, column: next.column };
    viewport.revealRow(next.row);
  };

  React.useEffect(() => {
    const pending = pendingFocus.current;
    if (pending === null) return;
    if (focusSheetCell(document, rows, columns, pending)) pendingFocus.current = null;
  }, [columns, rows, viewport.end, viewport.start]);

  const toggleExpanded = (row: DisplayRow): void => {
    if (row.depth >= 3) {
      setPeekIssue(row.issue);
      return;
    }
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(row.issue.id)) next.delete(row.issue.id);
      else next.add(row.issue.id);
      return next;
    });
  };

  if (query.isPending) return <SpreadsheetSkeleton />;

  const columnCount = columns.length + 1;
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div
        ref={setScrollNode}
        data-testid="spreadsheet-scroller"
        className="min-h-0 flex-1 overflow-auto"
        onScroll={(event) => setScrolled(event.currentTarget.scrollLeft > 0)}
      >
        <table className="w-max min-w-full border-collapse bg-(--bg) text-left">
          <thead className="sticky top-0 z-20">
            <tr>
              <th
                scope="col"
                tabIndex={-1}
                style={{ boxShadow: scrolled ? "8px 22px 22px 10px rgba(0, 0, 0, 0.05)" : "none" }}
                className="text-caption sticky left-0 z-30 h-11 min-w-60 border-r border-b border-(--border) bg-(--surface) px-(--space-3) font-medium text-(--text)"
              >
                <span>Work items</span>
              </th>
              {columns.map((column) => (
                <SheetHeaderCell
                  key={column.key}
                  column={column}
                  sorted={activeSort?.column === column.key ? activeSort : null}
                  onSort={(direction) => {
                    const orderBy = sheetSortOrder(column, direction);
                    const next = { column: column.key, direction, orderBy };
                    writeStoredSheetSort(next);
                    setLocalSort(next);
                    // Re-picking the server's current order only refreshes
                    // the local pair (which distinguishes created-on
                    // ascending from the unsorted default); it does not
                    // rewrite the same key.
                    if (prefs.orderBy !== orderBy) updateDisplayPrefs({ orderBy });
                  }}
                  onClear={() => {
                    writeStoredSheetSort(null);
                    setLocalSort(null);
                    updateDisplayPrefs({ orderBy: SHEET_DEFAULT_ORDER });
                  }}
                />
              ))}
            </tr>
          </thead>
          <tbody>
            {viewport.start > 0 ? <SpacerRow height={viewport.start * ROW_HEIGHT} columnCount={columnCount} /> : null}
            {visibleRows.map((row, offset) => {
              const rowIndex = viewport.start + offset;
              return (
                <SheetRow
                  key={row.issue.id}
                  row={row}
                  columns={columns}
                  rowIndex={rowIndex}
                  projectIdentifier={projectIdentifier}
                  scrolled={scrolled}
                  context={context}
                  highlighted={row.issue.id === highlightedId}
                  hasChildren={(row.issue.sub_issues_count ?? 0) > 0}
                  expanded={expanded.has(row.issue.id)}
                  onToggle={() => toggleExpanded(row)}
                  onOpen={() => openWorkItem(row.issue)}
                  onKeyDown={onGridKeyDown}
                />
              );
            })}
            {viewport.end < rows.length ? (
              <SpacerRow height={(rows.length - viewport.end) * ROW_HEIGHT} columnCount={columnCount} />
            ) : null}
          </tbody>
          <tfoot>
            {query.isFetchingNextPage ? (
              <>
                {[0, 1, 2].map((index) => (
                  <tr key={index} data-skeleton-row="true" className="h-11">
                    <td colSpan={columnCount} className="border-b border-(--border) px-(--space-3)">
                      <span className="block h-3 w-2/3 animate-pulse rounded-(--radius-control) bg-(--subtle)" />
                    </td>
                  </tr>
                ))}
              </>
            ) : null}
          </tfoot>
        </table>
      </div>
      <QuickAddRow
        projectIdentifier={projectIdentifier}
        disabled={!canEdit}
        pending={create.isPending}
        onCreate={(name) => create.mutateAsync({ name })}
      />
      {peekIssue === null ? null : (
        <PeekPanel
          eyebrow={`${projectIdentifier}-${peekIssue.sequence_id}`}
          title={peekIssue.name}
          titleLabel="Work item title"
          onClose={() => setPeekIssue(null)}
          actions={
            <CopyWorkItemLinkButton
              workspaceSlug={workspaceSlug}
              issueKey={`${projectIdentifier}-${peekIssue.sequence_id}`}
            />
          }
        >
          <Link
            to="/$ws/browse/$issueKey"
            params={{ ws: workspaceSlug, issueKey: `${projectIdentifier}-${peekIssue.sequence_id}` }}
            className="text-body font-medium text-(--accent) hover:underline"
          >
            Open the full work item
          </Link>
        </PeekPanel>
      )}
    </div>
  );
}

function arrowDirection(key: string): "up" | "down" | "left" | "right" | null {
  switch (key) {
    case "ArrowUp":
      return "up";
    case "ArrowDown":
      return "down";
    case "ArrowLeft":
      return "left";
    case "ArrowRight":
      return "right";
    default:
      return null;
  }
}

/**
 * Focus one grid cell by coordinates. Returns false when the row is gone
 * or the cell is outside the currently rendered window; the caller then
 * reveals the row and retries after the next render.
 */
export function focusSheetCell(
  root: Document,
  rows: readonly DisplayRow[],
  columns: readonly SheetColumn[],
  cell: { row: number; column: number }
): boolean {
  const row = rows[cell.row];
  if (row === undefined) return false;
  const id =
    cell.column === 0 ? sheetFirstCellId(row.issue.id) : `sheet-cell-${row.issue.id}-${columns[cell.column - 1]!.key}`;
  const target = root.getElementById(id);
  if (target === null) return false;
  target.focus();
  return true;
}

/** Depth-first display rows: roots plus expanded children to three levels. */
export function buildDisplayRows(
  roots: readonly IssueListItem[],
  expanded: ReadonlySet<string>,
  childrenByParent: ReadonlyMap<string, readonly IssueListItem[]>,
  depth = 0
): DisplayRow[] {
  const rows: DisplayRow[] = [];
  for (const issue of roots) {
    rows.push({ issue, depth });
    if (expanded.has(issue.id)) {
      rows.push(...buildDisplayRows(childrenByParent.get(issue.id) ?? [], expanded, childrenByParent, depth + 1));
    }
  }
  return rows;
}

/**
 * Fixed-row-height windowing: the visible slice plus two rows of
 * overscan. Recomputes on scroll and on viewport resize; reports when
 * the bottom edge is reached so the caller can request the next page.
 */
export function useViewportWindow(
  node: HTMLElement | null,
  rowHeight: number,
  rowCount: number,
  overscan = 4
): { start: number; end: number; atBottom: boolean; revealRow: (row: number) => void } {
  const [scrollTop, setScrollTop] = React.useState(0);
  const [height, setHeight] = React.useState(0);
  React.useEffect(() => {
    if (node === null) return;
    const update = (): void => {
      setScrollTop(node.scrollTop);
      setHeight(node.clientHeight);
    };
    update();
    node.addEventListener("scroll", update, { passive: true });
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
    observer?.observe(node);
    return () => {
      node.removeEventListener("scroll", update);
      observer?.disconnect();
    };
  }, [node]);
  const visible = Math.max(1, Math.ceil(height / rowHeight));
  const start = Math.max(0, Math.min(rowCount, Math.floor(scrollTop / rowHeight) - overscan));
  const end = Math.max(start, Math.min(rowCount, start + visible + overscan * 2));
  const atBottom = height > 0 && scrollTop + height >= rowCount * rowHeight - rowHeight * 2;
  const revealRow = (row: number): void => {
    if (node === null) return;
    const top = row * rowHeight;
    const viewTop = node.scrollTop;
    const viewBottom = viewTop + node.clientHeight;
    if (top >= viewTop && top + rowHeight <= viewBottom) return;
    const centered = Math.max(0, top - Math.floor(node.clientHeight / 2));
    const maxTop = Math.max(0, rowCount * rowHeight - node.clientHeight);
    const nextTop = Math.min(centered, maxTop);
    node.scrollTop = nextTop;
    setScrollTop(nextTop);
  };
  return { start, end, atBottom, revealRow };
}

function SpacerRow({ height, columnCount }: { height: number; columnCount: number }): React.ReactElement {
  return (
    <tr aria-hidden="true">
      <td colSpan={columnCount} style={{ height: `${height}px`, padding: 0, border: 0 }} />
    </tr>
  );
}

function SheetHeaderCell({
  column,
  sorted,
  onSort,
  onClear,
}: {
  column: SheetColumn;
  sorted: SheetSortState | null;
  onSort: (direction: SheetSortDirection) => void;
  onClear: () => void;
}): React.ReactElement {
  const items = [
    { label: SHEET_SORT_ASCENDING_LABEL, onSelect: () => onSort("ascending") },
    { label: SHEET_SORT_DESCENDING_LABEL, onSelect: () => onSort("descending") },
    ...(sorted === null ? [] : [{ label: SHEET_SORT_CLEAR_LABEL, onSelect: onClear, icon: <Eraser size={12} /> }]),
  ];
  return (
    <th
      scope="col"
      tabIndex={0}
      className="text-caption h-11 min-w-36 border-r border-b border-(--border) bg-(--surface) py-1 font-medium text-(--text)"
    >
      <Menu
        trigger={
          <button
            type="button"
            className="flex h-full w-full cursor-pointer items-center justify-between gap-(--space-2) px-(--space-3) text-left hover:bg-(--subtle)"
          >
            <span>{column.label}</span>
            <span className="flex items-center">
              {sorted === null ? null : (
                <div className="flex size-3.5 items-center justify-center rounded-full">
                  {sorted.direction === "ascending" ? (
                    <ArrowDownWideNarrow size={12} strokeWidth={1.5} aria-hidden="true" />
                  ) : (
                    <ArrowUpNarrowWide size={12} strokeWidth={1.5} aria-hidden="true" />
                  )}
                </div>
              )}
              <ChevronRight size={12} strokeWidth={1.5} aria-hidden="true" className="rotate-90 text-(--text-muted)" />
            </span>
          </button>
        }
        items={items}
      />
    </th>
  );
}

function SheetRow({
  row,
  columns,
  rowIndex,
  projectIdentifier,
  scrolled,
  context,
  highlighted,
  hasChildren,
  expanded,
  onToggle,
  onOpen,
  onKeyDown,
}: {
  row: DisplayRow;
  columns: readonly SheetColumn[];
  rowIndex: number;
  projectIdentifier: string;
  scrolled: boolean;
  context: SheetCellContext;
  highlighted: boolean;
  hasChildren: boolean;
  expanded: boolean;
  onToggle: () => void;
  onOpen: () => void;
  onKeyDown: (event: React.KeyboardEvent<HTMLTableCellElement>) => void;
}): React.ReactElement {
  const { issue, depth } = row;
  const temp = issue.id.startsWith("temp-");
  const key = temp ? "…" : `${projectIdentifier}-${issue.sequence_id}`;
  const href = `/${context.workspaceSlug}/browse/${key}`;
  const cellStyle = { boxShadow: scrolled ? "8px 22px 22px 10px rgba(0, 0, 0, 0.05)" : "none" };
  return (
    <tr
      data-highlight={highlighted ? "true" : undefined}
      data-temprow={temp ? "true" : undefined}
      className={highlighted ? "bg-(--accent-subtle)" : undefined}
    >
      <td
        id={`issue-${issue.id}`}
        tabIndex={0}
        data-row-index={rowIndex}
        data-column-index={0}
        onKeyDown={onKeyDown}
        style={cellStyle}
        className="text-caption sticky left-0 z-10 h-11 max-w-lg min-w-60 border-r border-b border-(--border) bg-(--bg) px-(--space-3) text-(--text)"
      >
        <div className="flex h-full w-full items-center gap-(--space-2)">
          <div style={{ width: `${depth * 12}px` }} aria-hidden="true" />
          <span className="grid size-4 shrink-0 place-items-center">
            {hasChildren && !temp ? (
              <button
                type="button"
                aria-label={expanded ? "Collapse sub-work items" : "Expand sub-work items"}
                onClick={onToggle}
                className="grid size-4 cursor-pointer place-items-center rounded-(--radius-control) text-(--text-muted) hover:text-(--text)"
              >
                <ChevronRight
                  size={14}
                  strokeWidth={2}
                  aria-hidden="true"
                  className={expanded ? "rotate-90" : undefined}
                />
              </button>
            ) : null}
          </span>
          <button
            type="button"
            disabled
            className="shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted) disabled:cursor-default"
          >
            {key}
          </button>
          <div className="min-w-0 flex-1 truncate">
            {temp ? (
              <span>{issue.name}</span>
            ) : (
              <Link to={href} className="text-(--text) hover:underline">
                {issue.name}
              </Link>
            )}
          </div>
          <button
            type="button"
            aria-label={`Open ${issue.name}`}
            onClick={onOpen}
            className="grid size-5 shrink-0 cursor-pointer place-items-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
          >
            <MoreHorizontal size={14} strokeWidth={1.5} aria-hidden="true" />
          </button>
        </div>
        {temp ? (
          <span aria-hidden="true" className="pointer-events-none absolute inset-0 animate-pulse bg-(--subtle)/60" />
        ) : null}
      </td>
      {columns.map((column, index) => (
        <SheetPropertyCell
          key={column.key}
          issue={issue}
          column={column}
          context={context}
          rowIndex={rowIndex}
          columnIndex={index + 1}
          onKeyDown={onKeyDown}
        />
      ))}
    </tr>
  );
}

const QUICK_ADD_PLACEHOLDER = "Work item title";

function QuickAddRow({
  projectIdentifier,
  disabled,
  pending,
  onCreate,
}: {
  projectIdentifier: string;
  disabled: boolean;
  pending: boolean;
  onCreate: (name: string) => Promise<unknown>;
}): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const [name, setName] = React.useState("");
  const inputRef = React.useRef<HTMLInputElement>(null);
  const containerRef = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);

  React.useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent): void => {
      if (event.target instanceof Node && containerRef.current?.contains(event.target) === false) {
        setOpen(false);
        setName("");
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [open]);

  const submit = async (): Promise<void> => {
    const trimmed = name.trim();
    if (trimmed === "") return;
    setName("");
    await onCreate(trimmed).catch(() => undefined);
    inputRef.current?.focus();
  };

  return (
    <div ref={containerRef} className="shrink-0 border-t border-(--border) bg-(--surface) p-(--space-2)">
      <Button
        variant="ghost"
        size="small"
        disabled={disabled || pending}
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        className="w-full justify-start"
      >
        <Plus size={14} strokeWidth={2} aria-hidden="true" />
        Add work item
      </Button>
      {open ? (
        <form
          className="mt-(--space-2) flex items-center gap-(--space-2)"
          onSubmit={(event) => {
            event.preventDefault();
            void submit();
          }}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              setOpen(false);
              setName("");
            }
          }}
        >
          <span className="text-caption w-20 shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)">
            {projectIdentifier}
          </span>
          <input
            ref={inputRef}
            aria-label={QUICK_ADD_PLACEHOLDER}
            placeholder={QUICK_ADD_PLACEHOLDER}
            value={name}
            onChange={(event) => setName(event.target.value)}
            autoComplete="off"
            className="text-body h-8 min-w-0 flex-1 rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) text-(--text)"
          />
          {pending ? <Spinner size="small" label="Creating work item" /> : null}
          <Button type="submit" variant="primary" size="small" disabled={pending}>
            Add
          </Button>
        </form>
      ) : null}
    </div>
  );
}

export { isEditableColumn };
