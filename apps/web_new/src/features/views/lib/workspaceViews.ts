// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace views list model (parity/specs/views.md §9, VIEW-029-032/039).
// Pure: the screen owns the read and the search input, this module owns the
// static defaults, the name match, the row order, the display cap, the
// hrefs and the tab title.

import type { SavedView } from "@pidash/api-client";

/** One of the four workspace views that exist without a stored record. */
export interface WorkspaceDefaultView {
  /** Static route key; also the last path segment of its deep link. */
  key: string;
  label: string;
}

/**
 * The four defaults, in the order the list renders them (VIEW-030). They
 * have no record, so nothing on the server can rename, reorder or remove
 * them; the labels are the app's own copy, not a record field.
 */
export const WORKSPACE_DEFAULT_VIEWS: readonly WorkspaceDefaultView[] = [
  { key: "all-issues", label: "All work items" },
  { key: "assigned", label: "Assigned" },
  { key: "created", label: "Created" },
  { key: "subscribed", label: "Subscribed" },
];

/** Custom name rows stop past this many characters (VIEW-032's cap). */
export const WORKSPACE_VIEW_NAME_LIMIT = 60;

/** Tab title for the workspace views surface (VIEW-029 pins the suffix). */
export function workspaceViewsTitle(workspaceName: string | undefined): string {
  if (workspaceName === undefined || workspaceName === "") return "All Views";
  return `${workspaceName} - All Views`;
}

/** Deep link to a static default (VIEW-030), e.g. `…/workspace-views/assigned`. */
export function workspaceDefaultViewHref(workspaceSlug: string, key: string): string {
  return `/${workspaceSlug}/workspace-views/${key}`;
}

/** Deep link to a stored workspace view (VIEW-032). */
export function workspaceCustomViewHref(workspaceSlug: string, viewId: string): string {
  return `/${workspaceSlug}/workspace-views/${viewId}`;
}

/** Escape a query for a case-insensitive literal substring match. */
function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * The search rule shared by both groups (VIEW-029): a case-insensitive
 * substring of the displayed name. A blank query keeps everything; a query
 * that matches nothing yields an empty group, and the screen hides it.
 */
export function matchesWorkspaceViewQuery(name: string, query: string): boolean {
  const needle = query.trim();
  if (needle === "") return true;
  return new RegExp(escapeRegExp(needle), "i").test(name);
}

/**
 * Cap a row's name with a trailing ellipsis (VIEW-032 leaves the length to
 * the slice). Never splits a surrogate pair in half: the cut walks back off
 * a lone high surrogate so an emoji stays whole.
 */
export function capWorkspaceViewName(name: string, limit: number = WORKSPACE_VIEW_NAME_LIMIT): string {
  if (name.length <= limit) return name;
  let end = limit - 1;
  const lastCode = name.charCodeAt(end - 1);
  if (lastCode >= 0xd800 && lastCode <= 0xdbff) end -= 1;
  return `${name.slice(0, end).trimEnd()}…`;
}

/** Newest first (VIEW-029). Stable: equal stamps keep the fetched order. */
export function sortWorkspaceViewsNewestFirst(views: readonly SavedView[]): SavedView[] {
  return [...views].sort((left, right) => right.created_at.localeCompare(left.created_at));
}

export interface WorkspaceViewGroups {
  defaults: WorkspaceDefaultView[];
  customs: SavedView[];
}

/**
 * The two groups after the name search. Customs keep display order; the
 * defaults keep their fixed order.
 */
export function selectWorkspaceViewGroups(customs: readonly SavedView[], query: string): WorkspaceViewGroups {
  return {
    defaults: WORKSPACE_DEFAULT_VIEWS.filter((view) => matchesWorkspaceViewQuery(view.label, query)),
    customs: customs.filter((view) => matchesWorkspaceViewQuery(view.name, query)),
  };
}

/**
 * What the customs area renders (VIEW-039): the skeleton while the read is
 * in flight, the inline retry on failure, and the rows otherwise — where an
 * empty list renders nothing at all because the defaults stand alone.
 */
export type WorkspaceCustomsState = "loading" | "error" | "rows";

export function resolveWorkspaceCustomsState(pending: boolean, failed: boolean): WorkspaceCustomsState {
  if (pending) return "loading";
  if (failed) return "error";
  return "rows";
}
