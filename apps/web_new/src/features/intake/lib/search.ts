// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The intake route's search schema (parity/specs/intake.md INT-001,
// INT-028). Two keys carry the whole screen state: which tab is open and
// which request is selected. Unknown keys are dropped, a malformed tab
// reads as the default, and a blank id counts as no selection. Pure, so
// the route, the auto-selection writer and the tests share it.

import { DEFAULT_INTAKE_TAB, parseIntakeTab, type IntakeTab } from "./tabs.js";

export interface IntakeSearch {
  currentTab: IntakeTab;
  inboxIssueId?: string;
}

/**
 * How a search write lands in history. User moves (selection, tab
 * switch, previous/next, triage advance) push so back/forward walks
 * them; corrections (settle, missing-id fallback) replace.
 */
export interface IntakeSearchWriteOptions {
  replace?: boolean;
}

/** The search value the screen writes for a tab and optional selection. */
export function intakeSearchFor(tab: IntakeTab, issueId: string | null | undefined): IntakeSearch {
  return issueId === null || issueId === undefined || issueId === ""
    ? { currentTab: tab }
    : { currentTab: tab, inboxIssueId: issueId };
}

/**
 * Validate the raw address search into the screen's two keys. A trimmed
 * non-empty id is kept; anything else drops the key. The tab is parsed
 * through the tab rule, so the result always names a valid tab.
 */
export function parseIntakeSearch(raw: unknown): IntakeSearch {
  const source = typeof raw === "object" && raw !== null ? (raw as Record<string, unknown>) : {};
  const tab = parseIntakeTab(source["currentTab"]);
  const candidate = source["inboxIssueId"];
  const issueId = typeof candidate === "string" ? candidate.trim() : "";
  return issueId === "" ? { currentTab: tab } : { currentTab: tab, inboxIssueId: issueId };
}

/** The search a URL that drops the selection keeps: its tab, no id. */
export function withoutSelection(search: IntakeSearch): IntakeSearch {
  return { currentTab: search.currentTab };
}

/**
 * What the address should say once a tab's list has settled (INT-005,
 * INT-028 step 3/4): keep a selection present in the list, otherwise
 * auto-select the first row, otherwise clear the selection. Returns the
 * search to write, or null when the address already says exactly that.
 */
export function settleIntakeSearch(args: {
  search: IntakeSearch;
  /** Row work-item ids of the active tab's first page, in display order. */
  rowIds: readonly string[];
}): IntakeSearch | null {
  const { search, rowIds } = args;
  const current = search.inboxIssueId;
  const next =
    current !== undefined && rowIds.includes(current) ? search : intakeSearchFor(search.currentTab, rowIds[0] ?? null);
  if (next.inboxIssueId === search.inboxIssueId && next.currentTab === search.currentTab) return null;
  return next;
}

/** Default search: the default tab, nothing selected. */
export const DEFAULT_INTAKE_SEARCH: IntakeSearch = { currentTab: DEFAULT_INTAKE_TAB };

/**
 * What the address should say after a request leaves the active tab's
 * list (INT-027/INT-028; the triage actions in slice E call this). The
 * neighbour takes the selection — the next row, or the previous row
 * when the removed row was last — and an emptied list keeps the tab
 * with no selection.
 */
export function intakeSearchAfterRemove(args: {
  search: IntakeSearch;
  /** Row work-item ids of the active tab, in display order, still holding the removed id. */
  rowIds: readonly string[];
  removedId: string;
}): IntakeSearch {
  const { search, rowIds, removedId } = args;
  const remaining = rowIds.filter((id) => id !== removedId);
  if (remaining.length === 0) return { currentTab: search.currentTab };
  const removedIndex = rowIds.indexOf(removedId);
  if (removedIndex === -1) return { currentTab: search.currentTab, inboxIssueId: remaining[0]! };
  const next = remaining[removedIndex] ?? remaining[remaining.length - 1]!;
  return { currentTab: search.currentTab, inboxIssueId: next };
}
