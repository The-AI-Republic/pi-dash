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

/** One rung of the peek stack, as the address spells it. */
export interface PeekSelection {
  issueId: string;
  /** Project the item reads from; the route project when same-project. */
  projectId: string;
  /** Depth: 0 for the panel a layout opened, deeper for nested children. */
  nestingLevel: number;
}

/** What a layout reports to its host: a selection, or null when closed. */
export type PeekRequest = PeekSelection | null;

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

/** The selection the address names; the project falls back to the route. */
export function peekSelectionFromSearch(search: Record<string, unknown>, routeProjectId: string): PeekSelection | null {
  const raw = search[PEEK_ISSUE_PARAM];
  if (typeof raw !== "string" || raw.trim() === "") return null;
  const project = search[PEEK_PROJECT_PARAM];
  return {
    issueId: raw,
    projectId: typeof project === "string" && project !== "" ? project : routeProjectId,
    nestingLevel: parseNestingLevel(search[PEEK_NESTING_PARAM]),
  };
}

/** Search carrying one selection, with a foreign project written out. */
export function peekSearchFor(
  search: Record<string, unknown>,
  selection: PeekSelection,
  routeProjectId: string
): Record<string, unknown> {
  const next = stripPeekParams(search);
  next[PEEK_ISSUE_PARAM] = selection.issueId;
  if (selection.projectId !== routeProjectId) next[PEEK_PROJECT_PARAM] = selection.projectId;
  if (selection.nestingLevel > 0) next[PEEK_NESTING_PARAM] = String(selection.nestingLevel);
  return next;
}

/** The live address as a plain record; empty when there is no document. */
export function peekSearchFromLocation(): Record<string, string> {
  const location = globalThis.window?.location;
  if (location === undefined) return {};
  return Object.fromEntries(new URLSearchParams(location.search).entries());
}

/**
 * Replace the address with the given search, keeping the path. Layouts
 * without a router (the calendar) write the peek selection this way so
 * the address never grows a history entry (ISS-167).
 */
export function replacePeekSearch(search: Record<string, unknown>): void {
  const window = globalThis.window;
  if (window === undefined) return;
  const url = new URL(window.location.href);
  url.search = new URLSearchParams(
    Object.entries(search).flatMap(([key, value]) => (typeof value === "string" ? [[key, value]] : []))
  ).toString();
  window.history.replaceState({}, "", url.toString());
}

/** The panel a layout opened, latched while its stacked child holds the address. */
export interface PeekRoot {
  peeked: string;
  peekProjectId: string | null;
}

/** Whether the address still names the panel this mount opened. */
export function peekNamesRoot(root: PeekRoot, peeked: string, peekProjectId: string | null): boolean {
  return root.peeked === peeked && root.peekProjectId === peekProjectId;
}

/**
 * Whether the address names the stacked child this mount opened. The
 * child's own project is written out only when it differs from the
 * route, so the same-project case spells the param as absent.
 */
export function peekNamesChild(
  child: PeekSelection | null,
  routeProjectId: string,
  peeked: string,
  peekProjectId: string | null
): boolean {
  if (child === null || child.issueId !== peeked) return false;
  return (child.projectId === routeProjectId ? null : child.projectId) === peekProjectId;
}
