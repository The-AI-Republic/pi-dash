// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project view row actions model (parity/specs/views.md §6, VIEW-012,
// VIEW-015–018, VIEW-020). Pure: who may star, edit, delete and
// publish, the menu order, and the list-cache patches the star, edit,
// delete and publish writes apply. The containers own the queries and
// the writes; this module owns the decisions.

import type { Favorite, SavedView, SavedViewList } from "@pidash/api-client";

import { ROLE_ADMIN, ROLE_MEMBER } from "../../../core/session/permissions.js";

/** Row menu entries in display order (VIEW-017 pins the OSS four). */
export type ProjectViewMenuItemId = "edit" | "open" | "copy" | "publish" | "delete";

/** The star shows for members and admins; guests never see it (VIEW-012). */
export function canStarProjectView(role: number | null): boolean {
  return role !== null && role >= ROLE_MEMBER;
}

/** Editing is offered only to the view's owner (VIEW-015/017). */
export function canEditProjectView(viewerId: string | null, view: Pick<SavedView, "owned_by">): boolean {
  return viewerId !== null && view.owned_by === viewerId;
}

/** Deleting is offered to the owner or a project admin (VIEW-016/017). */
export function canDeleteProjectView(
  viewerId: string | null,
  view: Pick<SavedView, "owned_by">,
  role: number | null
): boolean {
  if (canEditProjectView(viewerId, view)) return true;
  return role !== null && role >= ROLE_ADMIN;
}

/**
 * Menu entries in order: Edit for the owner; Open and Copy for everyone
 * who sees the row; Publish on cloud builds for the owner or an admin;
 * Delete for the owner or an admin (VIEW-017, VIEW-020).
 */
export function selectProjectViewMenuItems(input: {
  isOwner: boolean;
  isAdmin: boolean;
  canPublish: boolean;
}): ProjectViewMenuItemId[] {
  const items: ProjectViewMenuItemId[] = [];
  if (input.isOwner) items.push("edit");
  items.push("open", "copy");
  if (input.canPublish && (input.isOwner || input.isAdmin)) items.push("publish");
  if (input.isOwner || input.isAdmin) items.push("delete");
  return items;
}

/** The star's selected state; missing or null reads unstarred. */
export function isViewFavorited(isFavorite: SavedView["is_favorite"]): boolean {
  return isFavorite === true;
}

/** Next list cache with one row's star flipped (VIEW-012). */
export function flipViewFavorite(previous: SavedViewList | undefined, viewId: string): SavedViewList | undefined {
  if (previous === undefined) return undefined;
  return previous.map((row) => (row.id === viewId ? { ...row, is_favorite: !isViewFavorited(row.is_favorite) } : row));
}

/** Next list cache with one row replaced by its PATCH echo (VIEW-015). */
export function replaceProjectViewRow(previous: SavedViewList | undefined, echo: SavedView): SavedViewList | undefined {
  if (previous === undefined) return undefined;
  return previous.map((row) => (row.id === echo.id ? echo : row));
}

/** Next list cache without the deleted row (VIEW-016). */
export function removeProjectViewRow(previous: SavedViewList | undefined, viewId: string): SavedViewList | undefined {
  if (previous === undefined) return undefined;
  return previous.filter((row) => row.id !== viewId);
}

/** Next list cache with one row's publish anchor set or cleared (VIEW-020). */
export function setProjectViewAnchor(
  previous: SavedViewList | undefined,
  viewId: string,
  anchor: string | null
): SavedViewList | undefined {
  if (previous === undefined) return undefined;
  return previous.map((row) => (row.id === viewId ? { ...row, anchor } : row));
}

/** A detail record with its publish anchor set or cleared (VIEW-020). */
export function withViewAnchor(record: SavedView | undefined, anchor: string | null): SavedView | undefined {
  if (record === undefined) return undefined;
  return { ...record, anchor };
}

/**
 * The favorite row's own id for an unstar (VIEW-012): the remove call
 * keys on the favorite row, not the view. Null when the list carries
 * no view favorite for this view; the caller falls back to a re-read.
 */
export function findViewFavoriteId(favorites: readonly Favorite[], viewId: string): string | null {
  return favorites.find((row) => row.entity_type === "view" && row.entity_identifier === viewId)?.id ?? null;
}
