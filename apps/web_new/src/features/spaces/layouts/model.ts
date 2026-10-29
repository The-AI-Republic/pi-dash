// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public-board layout and paging model (parity/specs/space.md §3.4). Pure
// functions only: which layouts are offered, how a requested one settles,
// how each group's first page becomes renderable groups, and how a later
// page appends to one group without touching the others.

import type { SpaceBoardIssue, SpaceBoardIssuePage, SpaceBoardIssueQuery, SpaceBoardState } from "@pidash/api-client";

import type { SpaceBoardFilters, SpaceLayout } from "../lib/address.js";
import { SPACE_LAYOUTS, facetValuesOf } from "../lib/address.js";

/** Rows requested per group, first and later pages alike. */
export const SPACE_BOARD_PAGE_SIZE = 50;

/** The literal first-page cursor the board sends (group offset 0, page 0). */
export const SPACE_FIRST_GROUP_CURSOR = `${SPACE_BOARD_PAGE_SIZE}:0:0`;

/** Newest first unless the request says otherwise. */
export const SPACE_BOARD_ORDER = "-created_at";

/**
 * The layouts the switcher offers: the public board's renderable layouts
 * that the publisher enabled, in list-then-board order. A board with none
 * enabled (malformed settings) stays usable in list.
 */
export function enabledSpaceLayouts(viewProps: Record<string, boolean>): SpaceLayout[] {
  const offered = SPACE_LAYOUTS.filter((layout) => viewProps[layout] === true);
  return offered.length > 0 ? [...offered] : [SPACE_LAYOUTS[0]];
}

/** Settle a requested layout on the offered set (SPC-020/022). */
export function resolveSpaceLayout(requested: SpaceLayout | undefined, offered: readonly SpaceLayout[]): SpaceLayout {
  if (requested !== undefined && offered.includes(requested)) return requested;
  return offered[0] ?? SPACE_LAYOUTS[0];
}

/** A stable identity for one filter combination, used for caches and resets. */
export function spaceFilterSignature(filters: SpaceBoardFilters): string {
  return [filters.state ?? "", filters.priority ?? "", filters.labels ?? ""].join("|");
}

/**
 * One board request. Without `state` it asks for a page of every group; with
 * it, for one group's page. The grouping stays on so both shapes parse the
 * same way, and the filtered request reports that group's own
 * `next_page_results`.
 */
export function spaceBoardQuery(
  filters: SpaceBoardFilters,
  cursor: string = SPACE_FIRST_GROUP_CURSOR,
  state?: string
): SpaceBoardIssueQuery {
  const stateValues = state === undefined ? facetValuesOf(filters.state) : [state];
  const priorityValues = facetValuesOf(filters.priority);
  const labelValues = facetValuesOf(filters.labels);
  return {
    groupBy: "state_id",
    perPage: SPACE_BOARD_PAGE_SIZE,
    cursor,
    orderBy: SPACE_BOARD_ORDER,
    ...(stateValues.length === 0 ? {} : { state: stateValues }),
    ...(priorityValues.length === 0 ? {} : { priority: priorityValues }),
    ...(labelValues.length === 0 ? {} : { labels: labelValues }),
  };
}

/** Whether this page offers a cursor and the group still has rows behind it. */
function pageHasNext(
  page: SpaceBoardIssuePage,
  loaded: number,
  total: number
): page is SpaceBoardIssuePage & {
  next_cursor: string;
} {
  return page.next_page_results && page.next_cursor !== null && loaded < total;
}

/** One renderable group: a workflow state, its loaded rows and its server total. */
export interface SpaceIssueGroup {
  stateId: string;
  state: SpaceBoardState;
  issues: SpaceBoardIssue[];
  /** The server's count for this group (the header number, SPC-023/024). */
  total: number;
  /** Cursor for the group's next page; null once it is exhausted. */
  nextCursor: string | null;
}

/** Workflow states in the board's order (sequence ascending, ties by name). */
export function orderedSpaceStates(states: readonly SpaceBoardState[]): SpaceBoardState[] {
  return [...states].sort((left, right) => {
    if (left.sequence !== right.sequence) return left.sequence - right.sequence;
    return left.name.localeCompare(right.name);
  });
}

/**
 * The first page, one group per workflow state in board order. States the
 * server did not report still appear, with zero rows and a zero count
 * (SPC-023/024).
 */
export function firstPageGroups(states: readonly SpaceBoardState[], page: SpaceBoardIssuePage): SpaceIssueGroup[] {
  return orderedSpaceStates(states).map((state) => {
    const bucket = page.results[state.id];
    const issues = bucket?.results ?? [];
    const total = bucket?.total_results ?? issues.length;
    return {
      stateId: state.id,
      state,
      issues,
      total,
      // The first page carries one shared cursor; only the groups that still
      // have unloaded rows may use it (SPC-026). An empty group never pages.
      nextCursor: pageHasNext(page, issues.length, total) ? page.next_cursor : null,
    };
  });
}

/** Append a later page to one group, dropping rows already shown (no duplicates). */
export function appendGroupPage(group: SpaceIssueGroup, page: SpaceBoardIssuePage): SpaceIssueGroup {
  const bucket = page.results[group.stateId];
  const known = new Set(group.issues.map((issue) => issue.id));
  const added = (bucket?.results ?? []).filter((issue) => !known.has(issue.id));
  const total = bucket?.total_results ?? group.total;
  const nextCursor = pageHasNext(page, group.issues.length + added.length, total) ? page.next_cursor : null;
  return {
    ...group,
    issues: added.length === 0 ? group.issues : [...group.issues, ...added],
    total,
    nextCursor,
  };
}

/** The group a paging request targets, located by state id. */
export function findGroup(groups: readonly SpaceIssueGroup[], stateId: string): SpaceIssueGroup | undefined {
  return groups.find((group) => group.stateId === stateId);
}

/** Whether a group still has rows the visitor has not loaded. */
export function groupHasNextPage(group: SpaceIssueGroup): boolean {
  return group.nextCursor !== null;
}
