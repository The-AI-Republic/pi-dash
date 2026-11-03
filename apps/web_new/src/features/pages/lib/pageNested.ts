// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Nested page model (parity/specs/pages.md §11.7, §12.2, PAGE-084–090).
// D17 turns the project's page list into a forest: the collection read stays
// top-level only and each expanded parent contributes one `?parent=` read.
// This module holds every decision the tree, the breadcrumb trail, the
// move picker and the delete confirmation make before they render. Pure, so
// the list, the detail header, the dialogs and their tests share one
// reading of parenthood, ancestry and move legality.

import type { PageRow } from "@pidash/api-client";

import { pageDisplayTitle } from "./pageList.js";

/** One visible ancestor of an open page, nearest first in `parentId`. */
export interface PageAncestor {
  id: string;
  /** Visible name, or the restricted fallback when the caller cannot see it. */
  name: string;
  /** The caller can resolve this ancestor; only then is it a link. */
  visible: boolean;
}

/** Fallback label for an ancestor the caller cannot see (pinned). */
export const PAGE_RESTRICTED_ANCESTOR = "Restricted page";

/** Label of the sub-page creation action in both menus (pinned). */
export const PAGE_ACTION_ADD_SUB_PAGE = "Add sub-page";

/** The move picker's menu entry, distinct from the cross-project move. */
export const PAGE_ACTION_MOVE_TO = "Move to…";

/** Dialog title of the within-project move. */
export const PAGE_MOVE_DIALOG_TITLE = "Move page";

/** Where a move puts the page: under another page, or at the top level. */
export const PAGE_MOVE_TOP_LEVEL = "Top level";

/** Title of the picker's top-level row. */
export const PAGE_MOVE_TOP_LEVEL_HINT = "Project root";

/** Notice after a move that carried a subtree. */
export const PAGE_NESTED_MOVED_TITLE = "Page moved.";

/** Notice when a restore detaches a page from an archived parent. */
export const PAGE_RESTORE_TO_TOP_TITLE = "Page restored to the top level.";

/** Notice when a move is refused before it reaches the server. */
export const PAGE_MOVE_REFUSED_TITLE = "Page could not be moved.";

/** Why a target page cannot receive the moved page. */
export type PageMoveRefusal = "self" | "descendant" | "archived" | "missing";

/** The parent id a row carries, preferring the flat field when present. */
export function pageParentId(row: Pick<PageRow, "parent" | "parent_id">): string | null {
  if (row.parent_id !== undefined) return row.parent_id;
  return row.parent;
}

/** Visible, non-archived children the caller may count (PAGE-085). */
export function pageChildCount(row: Pick<PageRow, "sub_pages_count">): number {
  return row.sub_pages_count ?? 0;
}

/**
 * Whether a row can still have children worth reading. `sub_pages_count`
 * counts live children only, so a cascaded archive leaves an archived
 * parent reporting zero even though its shelved subtree exists (PAGE-088).
 * Discovery therefore treats every archived row as potentially expandable
 * until its `?parent=` read settles, while live rows follow the count.
 */
export function pageMayHaveChildren(row: Pick<PageRow, "sub_pages_count" | "archived_at">): boolean {
  return pageChildCount(row) > 0 || row.archived_at !== null;
}

/**
 * The children of one parent inside a pool of rows. The pool is whatever
 * collection reads the caller has; rows are ordered by the list's sort
 * elsewhere, so this only partitions.
 */
export function childrenOf(rows: readonly PageRow[], parentId: string): PageRow[] {
  return rows.filter((row) => pageParentId(row) === parentId);
}

/**
 * One rendered tree line. `depth` counts ancestors above it (0 = top
 * level); `hasChildren` drives the expand control even before the children
 * are loaded, and `expanded` reports the current state.
 */
export interface PageTreeEntry {
  page: PageRow;
  depth: number;
  hasChildren: boolean;
  /** Direct children to name in the expand control and delete dialog. */
  childCount: number;
  expanded: boolean;
  /** The children read for this parent is still in flight. */
  loading: boolean;
}

/** How one parent's children read behaves in the tree. */
export interface PageTreeExpansion {
  expanded: ReadonlySet<string>;
  /** Children by parent id, for parents already read. */
  children: ReadonlyMap<string, readonly PageRow[]>;
  pending: ReadonlySet<string>;
}

/**
 * Flatten the forest into indented lines. Only expanded parents contribute
 * children, and a child that is itself expanded recurses. Rows keep the
 * order they arrive in, so the caller sorts each sibling list first.
 */
export function flattenPageTree(
  roots: readonly PageRow[],
  expansion: PageTreeExpansion,
  depth = 0,
  seen: ReadonlySet<string> = new Set()
): PageTreeEntry[] {
  const entries: PageTreeEntry[] = [];
  for (const page of roots) {
    // A malformed cycle in server data must not hang the list.
    if (seen.has(page.id)) continue;
    const nextSeen = new Set(seen).add(page.id);
    const expanded = expansion.expanded.has(page.id);
    const loaded = expansion.children.get(page.id)?.length ?? 0;
    // `sub_pages_count` is the contract's visible, non-archived count. The
    // archived tab loads its shelved subtree through `?parent=` reads, where
    // that field is zero, so the loaded children supply its count instead.
    const childCount = Math.max(pageChildCount(page), loaded);
    entries.push({
      page,
      depth,
      hasChildren: childCount > 0,
      childCount,
      expanded,
      loading: expansion.pending.has(page.id),
    });
    if (!expanded) continue;
    const kids = expansion.children.get(page.id) ?? [];
    entries.push(...flattenPageTree(kids, expansion, depth + 1, nextSeen));
  }
  return entries;
}

/**
 * Ids of every descendant of one page inside the rows the caller already
 * read, at any depth. The move rules and the picker use it to refuse a
 * cycle (PAGE-087).
 */
export function descendantIds(rows: readonly PageRow[], pageId: string): Set<string> {
  const found = new Set<string>();
  const queue = [pageId];
  while (queue.length > 0) {
    const current = queue.shift() as string;
    for (const row of rows) {
      if (pageParentId(row) !== current || found.has(row.id)) continue;
      found.add(row.id);
      queue.push(row.id);
    }
  }
  return found;
}

/**
 * Whether one page may be moved under `targetId` (null = top level).
 * Refusals, in the spec's order: the page itself, one of its descendants,
 * an archived page, or a target the caller cannot see (absent from the
 * pool). Moving to the page's current parent is legal but a no-op, so the
 * picker can offer it as the current selection.
 */
export function pageMoveRefusal(
  rows: readonly PageRow[],
  pageId: string,
  targetId: string | null
): PageMoveRefusal | null {
  if (targetId === null) return null;
  if (targetId === pageId) return "self";
  if (descendantIds(rows, pageId).has(targetId)) return "descendant";
  const target = rows.find((row) => row.id === targetId);
  if (target === undefined) return "missing";
  if (target.archived_at !== null) return "archived";
  return null;
}

/** Human message for a refused move. */
export function pageMoveRefusalMessage(refusal: PageMoveRefusal): string {
  switch (refusal) {
    case "self":
      return "A page cannot be moved under itself.";
    case "descendant":
      return "A page cannot be moved under one of its own sub-pages.";
    case "archived":
      return "A page cannot be moved under an archived page.";
    case "missing":
      return "That page is no longer available.";
  }
}

/**
 * Rows the move picker offers: every live page the caller can see, minus
 * the page being moved and its whole subtree. The page's current parent
 * stays offered as the current selection; picking it is a legal no-op.
 */
export function pageMoveTargets(rows: readonly PageRow[], pageId: string): PageRow[] {
  const excluded = descendantIds(rows, pageId);
  return rows.filter((row) => row.id !== pageId && !excluded.has(row.id) && row.archived_at === null);
}

/**
 * The breadcrumb trail for a page, ordered top-down. It walks the loaded
 * rows from the open page up through `parent`, reversing at the end. An
 * ancestor missing from the pool is rendered as the restricted fallback:
 * the caller may read the child, but the server never handed over the
 * parent, which means the viewer cannot see it (PAGE-086). A gap ends the
 * walk because nothing above it can be resolved either.
 */
export function pageAncestors(rows: readonly PageRow[], pageId: string): PageAncestor[] {
  const byId = new Map(rows.map((row) => [row.id, row]));
  const trail: PageAncestor[] = [];
  const seen = new Set<string>([pageId]);
  let cursor = byId.get(pageId);
  let parentId = cursor === undefined ? null : pageParentId(cursor);
  while (parentId !== null && !seen.has(parentId)) {
    seen.add(parentId);
    const parent = byId.get(parentId);
    if (parent === undefined) {
      trail.push({ id: parentId, name: PAGE_RESTRICTED_ANCESTOR, visible: false });
      break;
    }
    trail.push({ id: parent.id, name: pageDisplayTitle(parent.name), visible: true });
    parentId = pageParentId(parent);
  }
  return trail.reverse();
}

/**
 * Whether the open page hangs under any ancestor the caller cannot see.
 * The breadcrumb trail is the only place this surfaces; the header still
 * opens the page itself.
 */
export function hasRestrictedAncestor(ancestors: readonly PageAncestor[]): boolean {
  return ancestors.some((ancestor) => !ancestor.visible);
}

/**
 * Secondary line for a search/favourite row: the trail of its parents,
 * outermost first. Returns null for a top-level page, and stops at the
 * first ancestor it cannot resolve.
 */
export function pageParentPath(rows: readonly PageRow[], pageId: string): string | null {
  const ancestors = pageAncestors(rows, pageId);
  if (ancestors.length === 0) return null;
  return ancestors.map((ancestor) => ancestor.name).join(" / ");
}

/**
 * How many children move to the top level when a page is deleted
 * (PAGE-089). The server re-homes direct children only, and the count in
 * the confirmation is that direct set.
 */
export function pageDeleteRehomedCount(row: Pick<PageRow, "sub_pages_count">): number {
  return pageChildCount(row);
}

/**
 * The direct-child count a caller may show without reading further: the
 * contract count for a live page, or undefined for an archived one, whose
 * zero count says nothing about its shelved subtree (PAGE-088/089). The
 * delete dialog resolves the undefined case from one `?parent=` read.
 */
export function pageDeleteKnownCount(row: Pick<PageRow, "sub_pages_count" | "archived_at">): number | undefined {
  return row.archived_at === null ? pageChildCount(row) : undefined;
}

/** Confirmation sentence naming the re-homed children; null for a leaf. */
export function pageDeleteRehomedMessage(count: number): string | null {
  if (count <= 0) return null;
  const noun = count === 1 ? "sub-page" : "sub-pages";
  return `${count} ${noun} will move to the top level.`;
}

/**
 * Whether restoring a page detaches it from an archived parent (PAGE-088).
 * The header calls this after the restore write, using the parent row from
 * the collection it already holds.
 */
export function restoreDetaches(rows: readonly PageRow[], page: Pick<PageRow, "parent" | "parent_id">): boolean {
  const parentId = pageParentId(page);
  if (parentId === null) return false;
  const parent = rows.find((row) => row.id === parentId);
  return parent !== undefined && parent.archived_at !== null;
}

/**
 * Filter a move-picker list by its search field, matching the visible
 * title and its parent path so a child is findable by either.
 */
export function filterMoveTargets(rows: readonly PageRow[], pool: readonly PageRow[], query: string): PageRow[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return [...rows];
  return rows.filter((row) => {
    const path = pageParentPath(pool, row.id) ?? "";
    return pageDisplayTitle(row.name).toLowerCase().includes(needle) || path.toLowerCase().includes(needle);
  });
}

/** One row the list renders before the tree decides its nesting. */
export interface PageNestedRow {
  page: PageRow;
  /** Ancestor trail, only when the row is shown outside its natural place. */
  parentPath: string | null;
}

/**
 * Rows the active tab shows at the top of the forest. A page hangs under its
 * parent only while that parent is also in the tab's set; an orphan (a child
 * whose parent is archived, or a page whose parent the caller cannot see)
 * becomes a root so it is never lost from the list.
 */
export function pageTreeRoots(
  rows: readonly PageRow[],
  tabRows: readonly PageRow[],
  sort: (rows: readonly PageRow[]) => PageRow[]
): PageRow[] {
  const present = new Set(tabRows.map((row) => row.id));
  const roots = tabRows.filter((row) => {
    const parentId = pageParentId(row);
    return parentId === null || !present.has(parentId);
  });
  return sort(roots);
}

/**
 * Discovery rows: when a search or a structured filter is active, matching
 * pages are shown as one flat result list across every depth, each carrying
 * its parent path (PAGE-090). Without a discovery dimension the list keeps
 * its tree shape and this returns null.
 */
export function pageDiscoveryRows(
  rows: readonly PageRow[],
  searchActive: boolean,
  filtersActive: boolean,
  select: (rows: readonly PageRow[]) => PageRow[]
): PageNestedRow[] | null {
  if (!searchActive && !filtersActive) return null;
  return select(rows).map((page) => ({ page, parentPath: pageParentPath(rows, page.id) }));
}
