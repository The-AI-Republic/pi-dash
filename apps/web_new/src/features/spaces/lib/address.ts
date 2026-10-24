// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public-board addresses (parity/specs/space.md §1.3). View state lives in
// the address; invalid values fall back instead of throwing, and legacy
// project links forward only the two parameters the old app kept. Filter
// keys ride along opaquely so the sign-in/sign-out return address keeps
// the full position (SPC-013, SPC-014); validating and settling them
// belongs to the filters slice, not to this parser.

/** Anchors are opaque server ids with this alphabet; anything else is unknown. */
export const ANCHOR_PATTERN = /^[A-Za-z0-9_-]+$/;

export const SPACE_BASE = "/spaces";
export const SPACE_BOARD_PREFIX = `${SPACE_BASE}/issues`;
/** The public board landing, the way back from a board. */
export const SPACE_LANDING_HREF = `${SPACE_BASE}/`;
export const SPACE_GENERIC_NOT_FOUND = "/404";

/**
 * The two layouts the public board can render, in the order the board
 * settles on one (list first). Anything else a publisher enabled — calendar,
 * gantt, spreadsheet — is never offered here.
 */
export const SPACE_LAYOUTS = ["list", "kanban"] as const;
export type SpaceLayout = (typeof SPACE_LAYOUTS)[number];

/** Priorities a board address may narrow by, highest first. */
export const SPACE_PRIORITIES = ["urgent", "high", "medium", "low", "none"] as const;

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** The filter facets a board address can carry (slice F owns their UI). */
export interface SpaceBoardFilters {
  state?: readonly string[] | undefined;
  priority?: readonly string[] | undefined;
  labels?: readonly string[] | undefined;
}

export interface SpaceBoardSearch extends SpaceBoardFilters {
  board?: SpaceLayout | undefined;
  peekId?: string | undefined;
}

export function isValidAnchor(anchor: string): boolean {
  return ANCHOR_PATTERN.test(anchor);
}

export function spaceBoardPath(anchor: string): string {
  return `${SPACE_BOARD_PREFIX}/${encodeURIComponent(anchor)}/`;
}

/** One issue's open address, keeping the board layout and every filter. */
export function spaceIssueHref(anchor: string, search: SpaceBoardSearch, issueId: string): string {
  return normalizeBoardHref(anchor, { ...search, peekId: issueId });
}

export function spaceLegacyPath(workspaceSlug: string, projectId: string): string {
  return `${SPACE_BASE}/${encodeURIComponent(workspaceSlug)}/${encodeURIComponent(projectId)}/`;
}

function stringParam(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() !== "" ? value : undefined;
}

/**
 * A comma-joined facet arrives as one string from a pasted address and as an
 * array from a validated search object; both shapes read the same way.
 */
function listParam(value: unknown): string[] | undefined {
  const raw =
    typeof value === "string" ? [value] : Array.isArray(value) ? value.filter((part) => typeof part === "string") : [];
  const parts = raw
    .flatMap((part) => part.split(","))
    .map((part) => part.trim())
    .filter((part) => part !== "");
  return parts.length === 0 ? undefined : parts;
}

function validLayout(value: string | undefined): SpaceLayout | undefined {
  return SPACE_LAYOUTS.find((layout) => layout === value);
}

/** Ids survive only when they look like ids; anything else is dropped. */
function validIds(value: unknown): string[] | undefined {
  const ids = listParam(value)?.filter((id) => UUID_PATTERN.test(id));
  return ids === undefined || ids.length === 0 ? undefined : ids;
}

function validPriorities(value: unknown): string[] | undefined {
  const levels = listParam(value)?.filter((level) => SPACE_PRIORITIES.some((known) => known === level));
  return levels === undefined || levels.length === 0 ? undefined : levels;
}

/**
 * Parse the board parameters this subtree understands. Unknown keys, unknown
 * layouts, malformed ids and unknown priorities are dropped; the layout
 * settles later against the publisher-enabled set (SPC-022).
 */
export function parseSpaceBoardSearch(search: Record<string, unknown>): SpaceBoardSearch {
  const board = validLayout(stringParam(search["board"]));
  const peekId = stringParam(search["peekId"]);
  const state = validIds(search["state"]);
  const priority = validPriorities(search["priority"]);
  const labels = validIds(search["labels"]);
  return {
    ...(board === undefined ? {} : { board }),
    ...(peekId === undefined ? {} : { peekId }),
    ...(state === undefined ? {} : { state }),
    ...(priority === undefined ? {} : { priority }),
    ...(labels === undefined ? {} : { labels }),
  };
}

function appendList(params: URLSearchParams, key: string, values: readonly string[] | undefined): void {
  if (values !== undefined && values.length > 0) params.set(key, values.join(","));
}

/**
 * The canonical board address for an anchor and settled search. Parameter
 * order is stable — board, peekId, then the filter facets — so a settled
 * address is byte-identical to a pasted one. `board` and `peekId` settle
 * through the parser; the filter facets echo byte for byte instead — the
 * settle rewrite keeps every other parameter that was present (SPC-022),
 * and the settled address is what the share and sign-in links carry
 * (SPC-013 keeps even an unsettled `state=abc`).
 */
export function normalizeBoardHref(anchor: string, search: SpaceBoardSearch): string {
  // The router merges keys the validator did not return back into the search,
  // so a facet here can still be the raw address string; echo it the same way
  // rather than trusting the caller's shape.
  const settled = parseSpaceBoardSearch({ ...search });
  const params = new URLSearchParams();
  if (settled.board !== undefined) params.set("board", settled.board);
  if (settled.peekId !== undefined) params.set("peekId", settled.peekId);
  appendList(params, "priority", listParam(search.priority));
  appendList(params, "state", listParam(search.state));
  appendList(params, "labels", listParam(search.labels));
  const query = params.toString();
  return `${spaceBoardPath(anchor)}${query === "" ? "" : `?${query}`}`;
}

/** Legacy links keep exactly `board` and `peekId`; filters are not forwarded. */
export function legacyForwardHref(anchor: string, search: SpaceBoardSearch | Record<string, unknown>): string {
  const parsed = parseSpaceBoardSearch({ ...search });
  return normalizeBoardHref(anchor, {
    ...(parsed.board === undefined ? {} : { board: parsed.board }),
    ...(parsed.peekId === undefined ? {} : { peekId: parsed.peekId }),
  });
}

/** Subtree-relative return address carried by sign-in links. */
export function spaceReturnPath(anchor: string, search: SpaceBoardSearch): string {
  const href = normalizeBoardHref(anchor, search);
  return href.slice(SPACE_BASE.length);
}

/**
 * Landing address for an anonymous sign-in link. The value stays
 * subtree-relative (no `/spaces` prefix) and keeps its path slashes literal,
 * so both the server and the oracle read the position unchanged.
 */
export function spaceSignInHref(anchor: string, search: SpaceBoardSearch): string {
  const encoded = encodeURIComponent(spaceReturnPath(anchor, search)).replaceAll("%2F", "/");
  return `${SPACE_BASE}/?next_path=${encoded}`;
}
