// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The intake route's search schema (parity/specs/intake.md INT-001,
// INT-028). Two keys carry the whole screen state: which tab is open and
// which request is selected. Unknown keys are dropped, a malformed tab
// reads as the default, and a blank id counts as no selection. The
// writers that compute the next address live in searchWrites.ts on the
// screen's chunk; this module stays eager for the route's validation.
// Pure, so the route, the prefetch and the tests share it.

import { parseIntakeTab, type IntakeTab } from "./tabs.js";

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
