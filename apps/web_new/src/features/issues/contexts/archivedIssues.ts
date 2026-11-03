// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived work items of one project (parity/specs/archives.md §2,
// ARCH-005–007). The archived tab keeps its expression, display options
// and visible columns in the window-local `issue_local_filters` record,
// never in the project's server user-properties; the oracle seeds that
// record directly, so reads and writes use the old entry shape. The list
// itself pages flat rows or per-group buckets and always sends an order.

import { keepPreviousData, queryOptions, useQueries, useQuery } from "@tanstack/react-query";
import {
  getArchivedIssue,
  listArchivedIssues,
  listGroupedArchivedIssues,
  type ApiClient,
  type ArchivedIssue,
  type GroupedIssueListResponse,
  type IssueListResponse,
} from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "../api/keys.js";
import { FLAT_PAGE_SIZE, GROUPED_PAGE_SIZE, ISSUES_PAGE_SIZE } from "../api/queries.js";
import { parseDisplayPrefs, type DisplayPrefs } from "../prefs/displayPrefs.js";
import type { RichExpression } from "../filters/rich/expression.js";
import { richFiltersParam, withRichFilters } from "../filters/rich/query.js";
import {
  flatSection,
  groupedSections,
  mergeGroupedPage,
  sectionHasMore,
  sectionNextCursor,
  serverGroupBy,
  type IssueGroupSection,
} from "../list/grouping.js";

/** Where the archived page's automation entry points. */
export function projectAutomationsPath(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/settings/projects/${projectId}/automations`;
}

/**
 * One archived work item's own address (parity/specs/archives.md §3.3).
 * The archived row and detail menus hand this out for copy-link and
 * open-in-new-tab; the live browse address is never used here.
 */
export function archivedIssueDetailPath(workspaceSlug: string, projectId: string, issueId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/archives/issues/${issueId}`;
}

/**
 * One flat page of a project's archived work items. The archived tab
 * always sends an order, so the first paint is manual rather than the
 * server's unqualified default (spec §2.1).
 */
export function archivedIssuesQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  expression: RichExpression,
  orderBy: string
) {
  return queryOptions({
    queryKey: [
      ...issueKeys.lists(workspaceSlug, "archived", projectId),
      orderBy,
      richFiltersParam(expression),
    ] as const,
    queryFn: (): Promise<IssueListResponse> =>
      listArchivedIssues(
        client,
        workspaceSlug,
        projectId,
        withRichFilters({ perPage: ISSUES_PAGE_SIZE, orderBy }, expression)
      ),
    staleTime: LIST_STALE_TIME_MS,
  });
}

export interface ArchivedPageInput {
  expression: RichExpression;
  /** Display dimension, or null for the flat list. */
  groupBy: string | null;
  orderBy: string;
  cursor?: string;
}

/** One archived page: flat or grouped at the display dimension. */
export function archivedPageQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  input: ArchivedPageInput
) {
  const serverKey = serverGroupBy(input.groupBy);
  return queryOptions({
    queryKey: [
      ...issueKeys.lists(workspaceSlug, "archived", projectId),
      "page",
      input.groupBy ?? "flat",
      input.orderBy,
      input.cursor ?? "first",
      richFiltersParam(input.expression),
    ] as const,
    queryFn: (): Promise<IssueListResponse | GroupedIssueListResponse> => {
      const query = withRichFilters(
        {
          perPage: serverKey === null ? FLAT_PAGE_SIZE : GROUPED_PAGE_SIZE,
          orderBy: input.orderBy,
          ...(input.cursor === undefined ? {} : { cursor: input.cursor }),
        },
        input.expression
      );
      return serverKey === null
        ? listArchivedIssues(client, workspaceSlug, projectId, query)
        : listGroupedArchivedIssues(client, workspaceSlug, projectId, { ...query, groupBy: serverKey });
    },
    staleTime: LIST_STALE_TIME_MS,
    placeholderData: keepPreviousData,
  });
}

/** One archived item's detail, read through its archive-scoped path. */
export function archivedIssueQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  issueId: string
) {
  return queryOptions({
    queryKey: [...issueKeys.detail(workspaceSlug, issueId), "archived"] as const,
    queryFn: (): Promise<ArchivedIssue> => getArchivedIssue(client, workspaceSlug, projectId, issueId),
    staleTime: LIST_STALE_TIME_MS,
  });
}

export interface ArchivedPages {
  sections: IssueGroupSection[];
  total: number;
  grouped: boolean;
  isPending: boolean;
  isError: boolean;
  refetch: () => void;
  hasMore: boolean;
  loadMore: () => void;
  loadMoreSection: (section: IssueGroupSection) => void;
  sectionMore: (section: IssueGroupSection) => boolean;
}

function isGroupedPage(page: IssueListResponse | GroupedIssueListResponse): page is GroupedIssueListResponse {
  return typeof page.results === "object" && page.results !== null && !Array.isArray(page.results);
}

/**
 * Page through the archived list. The flat cursor chain appends rows;
 * grouped reads keep one cursor chain per bucket so a small bucket does
 * not wait for the global window.
 */
export function useArchivedPages(
  workspaceSlug: string,
  projectId: string,
  expression: RichExpression,
  groupBy: string | null,
  orderBy: string,
  labels: ReadonlyMap<string, string>,
  showEmptyGroups: boolean
): ArchivedPages {
  const grouped = serverGroupBy(groupBy) !== null;
  const resetKey = `${groupBy ?? "flat"}|${orderBy}|${richFiltersParam(expression) ?? ""}`;
  const [flatCursors, setFlatCursors] = React.useState<Array<string | undefined>>([undefined]);
  const [sectionCursors, setSectionCursors] = React.useState<Record<string, string[]>>({});
  const [activeKey, setActiveKey] = React.useState(resetKey);
  if (activeKey !== resetKey) {
    setActiveKey(resetKey);
    setFlatCursors([undefined]);
    setSectionCursors({});
  }
  const client = getAppClient();
  const flatQueries = useQueries({
    queries: grouped
      ? []
      : flatCursors.map((cursor) =>
          archivedPageQueryOptions(client, workspaceSlug, projectId, {
            expression,
            groupBy: null,
            orderBy,
            ...(cursor === undefined ? {} : { cursor }),
          })
        ),
  });
  const sectionEntries = React.useMemo(
    () => Object.entries(sectionCursors).flatMap(([key, cursors]) => cursors.map((cursor) => ({ key, cursor }))),
    [sectionCursors]
  );
  const sectionQueries = useQueries({
    queries: grouped
      ? sectionEntries.map((entry) =>
          archivedPageQueryOptions(client, workspaceSlug, projectId, {
            expression,
            groupBy,
            orderBy,
            cursor: entry.cursor,
          })
        )
      : [],
  });
  const firstGrouped = useQuery({
    ...archivedPageQueryOptions(client, workspaceSlug, projectId, { expression, groupBy, orderBy }),
    enabled: grouped,
  });
  const flatPages = flatQueries.map((query) => query.data).filter((page) => page !== undefined);
  const firstGroupedPage = firstGrouped.data;
  const sections = React.useMemo(() => {
    if (!grouped) {
      // Every flat-chain page is an ungrouped envelope.
      const rows = flatPages.flatMap((page) => (page as IssueListResponse).results);
      const unique = new Map(rows.map((row) => [row.id, row]));
      return [flatSection([...unique.values()], flatPages[flatPages.length - 1]?.total_count ?? 0)];
    }
    let merged: IssueGroupSection[] = [];
    if (firstGroupedPage !== undefined && isGroupedPage(firstGroupedPage)) {
      merged = mergeGroupedPage(
        merged,
        groupedSections(firstGroupedPage, labels, { showEmptyGroups, dimension: groupBy })
      );
    }
    for (let index = 0; index < sectionEntries.length; index++) {
      const page = sectionQueries[index]?.data;
      if (page === undefined || !isGroupedPage(page)) continue;
      merged = mergeGroupedPage(merged, groupedSections(page, labels, { showEmptyGroups, dimension: groupBy }));
    }
    return merged;
  }, [flatPages, firstGroupedPage, groupBy, grouped, labels, sectionEntries, sectionQueries, showEmptyGroups]);
  const flatLast = flatPages[flatPages.length - 1];
  const total = grouped ? sections.reduce((sum, section) => sum + section.total, 0) : (flatLast?.total_count ?? 0);
  const loadMore = React.useCallback(() => {
    if (grouped || flatLast === undefined || !flatLast.next_page_results) return;
    const cursor = flatLast.next_cursor;
    if (cursor === "") return;
    setFlatCursors((current) => (current.includes(cursor) ? current : [...current, cursor]));
  }, [flatLast, grouped]);
  const loadMoreSection = React.useCallback((section: IssueGroupSection) => {
    if (!sectionHasMore(section)) return;
    // A section that still has rows always yields a cursor.
    const cursor = sectionNextCursor(section, GROUPED_PAGE_SIZE) as string;
    setSectionCursors((current) => {
      const existing = current[section.key] ?? [];
      if (existing.includes(cursor)) return current;
      return { ...current, [section.key]: [...existing, cursor] };
    });
  }, []);
  // The flat chain always holds its first cursor and the grouped read is
  // always mounted, so there is always a first query.
  const firstQuery = grouped ? firstGrouped : (flatQueries[0] as (typeof flatQueries)[number]);
  const refetch = React.useCallback(() => {
    void firstQuery.refetch();
  }, [firstQuery]);
  return {
    sections,
    total,
    grouped,
    isPending: firstQuery.isPending,
    isError: firstQuery.isError,
    refetch,
    hasMore: grouped ? false : (flatLast?.next_page_results ?? false),
    loadMore,
    loadMoreSection,
    sectionMore: sectionHasMore,
  };
}

/** Window-local store key the archived page restores its state from. */
export const ARCHIVED_FILTER_STORAGE_KEY = "issue_local_filters";

interface ArchivedFilterEntry {
  key: "ARCHIVED";
  workspaceSlug: string;
  viewId: string;
  filters: Record<string, unknown>;
}

/** The complete archived page state the local record carries. */
export interface ArchivedLocalState {
  expression: RichExpression;
  display: DisplayPrefs;
}

/** Display defaults for the archived tab: list, no grouping, manual order. */
export const DEFAULT_ARCHIVED_DISPLAY: DisplayPrefs = {
  layout: "list",
  groupBy: null,
  subGroupBy: null,
  orderBy: "sort_order",
  showEmptyGroups: false,
  showSubIssues: true,
  calendar: { mode: "month", showWeekends: false },
  properties: {},
};

/** Display keys the archived control owns inside `display_filters`. */
const OWNED_DISPLAY_KEYS = ["group_by", "order_by", "show_empty_groups", "sub_issue"] as const;

/**
 * Whether the page is showing the untouched archived state: no condition
 * and the default display. A pristine visit leaves no persisted entry
 * behind, because there is nothing the user chose to remember.
 */
export function isPristineArchivedState(expression: RichExpression, display: DisplayPrefs): boolean {
  return (
    Object.keys(expression).length === 0 &&
    display.layout === DEFAULT_ARCHIVED_DISPLAY.layout &&
    display.groupBy === DEFAULT_ARCHIVED_DISPLAY.groupBy &&
    display.subGroupBy === DEFAULT_ARCHIVED_DISPLAY.subGroupBy &&
    display.orderBy === DEFAULT_ARCHIVED_DISPLAY.orderBy &&
    display.showEmptyGroups === DEFAULT_ARCHIVED_DISPLAY.showEmptyGroups &&
    display.showSubIssues === DEFAULT_ARCHIVED_DISPLAY.showSubIssues &&
    Object.keys(display.properties).length === 0
  );
}

function record(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? (value as Record<string, unknown>) : {};
}

function expressionOrEmpty(value: unknown): RichExpression {
  return record(value);
}

function defaultStorage(): Storage | undefined {
  try {
    return window.localStorage;
  } catch {
    return undefined;
  }
}

function readEntries(storage: Storage): unknown[] {
  const raw = storage.getItem(ARCHIVED_FILTER_STORAGE_KEY);
  if (raw === null) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function matchingEntry(
  entries: readonly unknown[],
  workspaceSlug: string,
  projectId: string
): Partial<ArchivedFilterEntry> | null {
  for (const entry of entries) {
    if (typeof entry !== "object" || entry === null) continue;
    const candidate = entry as Partial<ArchivedFilterEntry>;
    if (candidate.key !== "ARCHIVED") continue;
    if (candidate.workspaceSlug !== workspaceSlug || candidate.viewId !== projectId) continue;
    if ((candidate as { userId?: unknown }).userId !== undefined) continue;
    return candidate;
  }
  return null;
}

/**
 * Read the archived expression stored for one project. A browser without
 * usable storage, a corrupt record, or an entry written for another
 * workspace or project all read as no expression.
 */
export function readArchivedLocalFilter(
  workspaceSlug: string,
  projectId: string,
  storage: Storage | undefined = defaultStorage()
): RichExpression {
  if (storage === undefined) return {};
  const entry = matchingEntry(readEntries(storage), workspaceSlug, projectId);
  return expressionOrEmpty(entry?.filters?.richFilters);
}

/** Read the archived display choices stored for one project. */
export function readArchivedLocalDisplay(
  workspaceSlug: string,
  projectId: string,
  storage: Storage | undefined = defaultStorage()
): DisplayPrefs {
  if (storage === undefined) return DEFAULT_ARCHIVED_DISPLAY;
  const entry = matchingEntry(readEntries(storage), workspaceSlug, projectId);
  const filters = record(entry?.filters);
  const parsed = parseDisplayPrefs(record(filters["display_filters"]), record(filters["display_properties"]));
  return { ...parsed, layout: "list", subGroupBy: null };
}

/** Read both halves of the archived page state in one storage pass. */
export function readArchivedLocalState(
  workspaceSlug: string,
  projectId: string,
  storage: Storage | undefined = defaultStorage()
): ArchivedLocalState {
  return {
    expression: readArchivedLocalFilter(workspaceSlug, projectId, storage),
    display: readArchivedLocalDisplay(workspaceSlug, projectId, storage),
  };
}

/**
 * Merge one project's archived state into the local record, keeping the
 * other projects and unknown members (`kanban_filters`, `calendar`, future
 * keys) untouched. A pristine state (no condition, default display) writes
 * nothing: any members this module owns are dropped, and the entry goes
 * away when nothing else remains, so a first untouched visit leaves no
 * record behind.
 */
export function writeArchivedLocalState(
  workspaceSlug: string,
  projectId: string,
  expression: RichExpression,
  display: DisplayPrefs,
  storage: Storage | undefined = defaultStorage()
): void {
  if (storage === undefined) return;
  const entries = readEntries(storage);
  const existing = matchingEntry(entries, workspaceSlug, projectId);
  const kept = entries.filter((entry) => {
    if (typeof entry !== "object" || entry === null) return false;
    const candidate = entry as Partial<ArchivedFilterEntry>;
    return !(
      candidate.key === "ARCHIVED" &&
      candidate.workspaceSlug === workspaceSlug &&
      candidate.viewId === projectId &&
      (candidate as { userId?: unknown }).userId === undefined
    );
  });
  const filters: Record<string, unknown> = { ...record(existing?.filters) };
  if (isPristineArchivedState(expression, display)) {
    // A pristine page remembers nothing of its own; members it never set
    // (`layout`, `calendar`, ...) survive on the stored entry.
    delete filters["richFilters"];
    delete filters["display_properties"];
    const stored = record(filters["display_filters"]);
    for (const key of OWNED_DISPLAY_KEYS) delete stored[key];
    if (Object.keys(stored).length > 0) filters["display_filters"] = stored;
    else delete filters["display_filters"];
  } else {
    if (Object.keys(expression).length === 0) delete filters["richFilters"];
    else filters["richFilters"] = expression;
    // Keep the members this page does not set (layout, calendar, ...) and
    // change only the choices the archived display owns.
    const displayFilters: Record<string, unknown> = { ...record(filters["display_filters"]) };
    if (display.groupBy === null) delete displayFilters["group_by"];
    else displayFilters["group_by"] = display.groupBy;
    displayFilters["order_by"] = display.orderBy;
    displayFilters["show_empty_groups"] = display.showEmptyGroups;
    displayFilters["sub_issue"] = display.showSubIssues;
    filters["display_filters"] = displayFilters;
    filters["display_properties"] = display.properties;
  }
  if (Object.keys(filters).length > 0) {
    kept.push({
      key: "ARCHIVED",
      workspaceSlug,
      viewId: projectId,
      filters,
    } satisfies ArchivedFilterEntry);
  }
  if (kept.length === 0) storage.removeItem(ARCHIVED_FILTER_STORAGE_KEY);
  else storage.setItem(ARCHIVED_FILTER_STORAGE_KEY, JSON.stringify(kept));
}

/**
 * Replace one project's stored archived expression, leaving any display
 * choices in the same entry untouched. Clearing the expression drops the
 * member and removes the entry only when nothing else remains.
 */
export function writeArchivedLocalFilter(
  workspaceSlug: string,
  projectId: string,
  expression: RichExpression,
  storage: Storage | undefined = defaultStorage()
): void {
  if (storage === undefined) return;
  const entries = readEntries(storage);
  const existing = matchingEntry(entries, workspaceSlug, projectId);
  const kept = entries.filter((entry) => {
    if (typeof entry !== "object" || entry === null) return false;
    const candidate = entry as Partial<ArchivedFilterEntry>;
    return !(
      candidate.key === "ARCHIVED" &&
      candidate.workspaceSlug === workspaceSlug &&
      candidate.viewId === projectId &&
      (candidate as { userId?: unknown }).userId === undefined
    );
  });
  const filters: Record<string, unknown> = { ...record(existing?.filters) };
  if (Object.keys(expression).length === 0) delete filters["richFilters"];
  else filters["richFilters"] = expression;
  if (Object.keys(filters).length > 0) {
    kept.push({
      key: "ARCHIVED",
      workspaceSlug,
      viewId: projectId,
      filters,
    } satisfies ArchivedFilterEntry);
  }
  if (kept.length === 0) storage.removeItem(ARCHIVED_FILTER_STORAGE_KEY);
  else storage.setItem(ARCHIVED_FILTER_STORAGE_KEY, JSON.stringify(kept));
}

/** Replace one project's stored archived display choices. */
export function writeArchivedLocalDisplay(
  workspaceSlug: string,
  projectId: string,
  display: DisplayPrefs,
  storage: Storage | undefined = defaultStorage()
): void {
  const expression = readArchivedLocalFilter(workspaceSlug, projectId, storage);
  writeArchivedLocalState(workspaceSlug, projectId, expression, display, storage);
}
