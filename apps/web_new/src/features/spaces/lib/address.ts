// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public-board addresses (parity/specs/space.md §1.3). View state lives in
// the address; invalid values fall back instead of throwing, and legacy
// project links forward only the two parameters the old app kept.

/** Anchors are opaque server ids with this alphabet; anything else is unknown. */
export const ANCHOR_PATTERN = /^[A-Za-z0-9_-]+$/;

export const SPACE_BASE = "/spaces";
export const SPACE_BOARD_PREFIX = `${SPACE_BASE}/issues`;
export const SPACE_GENERIC_NOT_FOUND = "/404";

export type SpaceLayout = "list" | "kanban";

export interface SpaceBoardSearch {
  board?: SpaceLayout | undefined;
  peekId?: string | undefined;
}

export function isValidAnchor(anchor: string): boolean {
  return ANCHOR_PATTERN.test(anchor);
}

export function spaceBoardPath(anchor: string): string {
  return `${SPACE_BOARD_PREFIX}/${encodeURIComponent(anchor)}/`;
}

export function spaceLegacyPath(workspaceSlug: string, projectId: string): string {
  return `${SPACE_BASE}/${encodeURIComponent(workspaceSlug)}/${encodeURIComponent(projectId)}/`;
}

function stringParam(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() !== "" ? value : undefined;
}

/** Parse only the board parameters this slice settles; unknown keys are dropped. */
export function parseSpaceBoardSearch(search: Record<string, unknown>): SpaceBoardSearch {
  const board = stringParam(search["board"]);
  const peekId = stringParam(search["peekId"]);
  return {
    ...(board === "list" || board === "kanban" ? { board } : {}),
    ...(peekId === undefined ? {} : { peekId }),
  };
}

/** The canonical board address for an anchor and settled search. */
export function normalizeBoardHref(anchor: string, search: SpaceBoardSearch): string {
  const params = new URLSearchParams();
  if (search.board !== undefined) params.set("board", search.board);
  if (search.peekId !== undefined) params.set("peekId", search.peekId);
  const query = params.toString();
  return `${spaceBoardPath(anchor)}${query === "" ? "" : `?${query}`}`;
}

/** Legacy links keep exactly `board` and `peekId`; filters are not forwarded. */
export function legacyForwardHref(anchor: string, search: SpaceBoardSearch | Record<string, unknown>): string {
  return normalizeBoardHref(anchor, parseSpaceBoardSearch({ ...search }));
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
