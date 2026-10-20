// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived work items of one project (parity/specs/issues.md §8, ISS-071).
// The issues slice owns only the empty card here: the archives area builds
// the tab shell, the read-only list and the row menus (NEWFRONT-698–700).
// This module supplies what the card needs — the read, the automation
// address, and the window-local expression store the archived page reads
// instead of the server record (NEWFRONT-167, pinned by the oracle).

import { queryOptions } from "@tanstack/react-query";
import { listArchivedIssues, type ApiClient, type IssueListResponse } from "@pidash/api-client";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { issueKeys } from "../api/keys.js";
import { ISSUES_PAGE_SIZE } from "../api/queries.js";
import type { RichExpression } from "../filters/rich/expression.js";
import { richFiltersParam, withRichFilters } from "../filters/rich/query.js";

/** Where the archived page's automation entry points. */
export function projectAutomationsPath(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/settings/projects/${projectId}/automations`;
}

/**
 * One project's archived work items. The read takes the same expression
 * and order the open page uses, because the empty card decides between
 * its two variants from the count this query returns.
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

/** Window-local store key the archived page restores its expression from. */
export const ARCHIVED_FILTER_STORAGE_KEY = "issue_local_filters";

/** One project's locally stored archived expression, as the page writes it. */
interface ArchivedFilterEntry {
  key: "ARCHIVED";
  workspaceSlug: string;
  viewId: string;
  filters: { richFilters: RichExpression };
}

function expressionOrEmpty(value: unknown): RichExpression {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? (value as RichExpression) : {};
}

function defaultStorage(): Storage | undefined {
  try {
    return typeof window === "undefined" ? undefined : window.localStorage;
  } catch {
    return undefined;
  }
}

/**
 * Read the archived expression stored for one project. A browser without
 * usable storage, a corrupt record, or an entry written for another
 * workspace or project all read as no expression: the list then shows
 * everything instead of inventing a filter.
 */
export function readArchivedLocalFilter(
  workspaceSlug: string,
  projectId: string,
  storage: Storage | undefined = defaultStorage()
): RichExpression {
  if (storage === undefined) return {};
  const raw = storage.getItem(ARCHIVED_FILTER_STORAGE_KEY);
  if (raw === null) return {};
  let entries: unknown;
  try {
    entries = JSON.parse(raw);
  } catch {
    return {};
  }
  if (!Array.isArray(entries)) return {};
  for (const entry of entries) {
    if (typeof entry !== "object" || entry === null) continue;
    const candidate = entry as Partial<ArchivedFilterEntry>;
    if (candidate.key !== "ARCHIVED") continue;
    if (candidate.workspaceSlug !== workspaceSlug || candidate.viewId !== projectId) continue;
    return expressionOrEmpty(candidate.filters?.richFilters);
  }
  return {};
}

/**
 * Replace one project's stored archived expression, keeping every other
 * entry (other projects, other local page state) untouched.
 */
export function writeArchivedLocalFilter(
  workspaceSlug: string,
  projectId: string,
  expression: RichExpression,
  storage: Storage | undefined = defaultStorage()
): void {
  if (storage === undefined) return;
  let entries: unknown[] = [];
  const raw = storage.getItem(ARCHIVED_FILTER_STORAGE_KEY);
  if (raw !== null) {
    try {
      const parsed: unknown = JSON.parse(raw);
      if (Array.isArray(parsed)) entries = parsed;
    } catch {
      entries = [];
    }
  }
  const kept = entries.filter((entry) => {
    if (typeof entry !== "object" || entry === null) return false;
    const candidate = entry as Partial<ArchivedFilterEntry>;
    return !(
      candidate.key === "ARCHIVED" &&
      candidate.workspaceSlug === workspaceSlug &&
      candidate.viewId === projectId
    );
  });
  if (Object.keys(expression).length > 0) {
    kept.push({
      key: "ARCHIVED",
      workspaceSlug,
      viewId: projectId,
      filters: { richFilters: expression },
    } satisfies ArchivedFilterEntry);
  }
  if (kept.length === 0) storage.removeItem(ARCHIVED_FILTER_STORAGE_KEY);
  else storage.setItem(ARCHIVED_FILTER_STORAGE_KEY, JSON.stringify(kept));
}
