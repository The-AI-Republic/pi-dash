// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Peek URL sync (parity/specs/issues.md §14, ISS-167). The selection
// two-way binds the peeked issue id, the peeked project id (omitted
// when it equals the route project), and the nesting level. Writers
// always replace, so browser Back leaves the page instead of closing
// the peek; closing strips every peek param.

export const PEEK_ISSUE_PARAM = "peekIssueId";
export const PEEK_PROJECT_PARAM = "peekProjectId";
export const PEEK_NESTING_PARAM = "peekNestingLevel";

/** Remove every peek param, keeping the rest of the search. */
export function stripPeekParams(search: Record<string, unknown>): Record<string, unknown> {
  const next = { ...search };
  delete next[PEEK_ISSUE_PARAM];
  delete next[PEEK_PROJECT_PARAM];
  delete next[PEEK_NESTING_PARAM];
  return next;
}

/** Search with the nesting level written; level 0 strips it. */
export function nestingSearchFor(search: Record<string, unknown>, level: number): Record<string, unknown> {
  const next = { ...search };
  if (level <= 0) delete next[PEEK_NESTING_PARAM];
  else next[PEEK_NESTING_PARAM] = String(level);
  return next;
}

/** Nesting depth from the URL; 0 when absent or invalid. */
export function parseNestingLevel(raw: unknown): number {
  if (typeof raw !== "string" && typeof raw !== "number") return 0;
  const level = typeof raw === "number" ? raw : Number.parseInt(raw, 10);
  if (!Number.isInteger(level) || level < 0) return 0;
  return level;
}
