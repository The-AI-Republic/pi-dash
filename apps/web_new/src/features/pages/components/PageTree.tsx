// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The nested page tree (parity/specs/pages.md §12.2, PAGE-085/087/090). The
// list's rows are the top level of a forest: a page that reports children
// carries an expand control and a visible count, expanding reads that one
// parent's children and indents them, and any expanded child recurses.
// Expansion lives in the in-session store, so leaving the list and coming
// back restores the same tree. A row can be dragged onto another page to
// reparent it with its whole subtree; the container itself is the drop
// target for the top level.

import { useQueries } from "@tanstack/react-query";
import type { PageRow as PageRowData } from "@pidash/api-client";
import { ChevronRight, GripVertical, Loader2 } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/toaster.js";
import { pageChildrenQueryOptions } from "../api/treeQueries.js";
import type { PageCreatorOption, PageListTab, PageSort } from "../lib/pageList.js";
import { pageInTab, sortPageSiblings } from "../lib/pageList.js";
import {
  flattenPageTree,
  pageMoveRefusal,
  pageMoveRefusalMessage,
  PAGE_MOVE_REFUSED_TITLE,
  type PageTreeEntry,
} from "../lib/pageNested.js";
import { pageDisplayTitle } from "../lib/pageList.js";
import { togglePageExpanded, usePageExpansion } from "../lib/pageTreeState.js";
import { PageRowConnected } from "./PageRowConnected.js";

/** Accessible label of one expand control. */
export function expandLabel(title: string, expanded: boolean): string {
  return `${expanded ? "Collapse" : "Expand"} sub-pages of ${title}`;
}

/** Accessible text of the child count badge; counts are visible (PAGE-085). */
export function childCountLabel(count: number): string {
  return count === 1 ? "1 sub-page" : `${count} sub-pages`;
}

/** Mime type used for a dragged page id inside the tree. */
export const PAGE_DRAG_MIME = "application/x-pidash-page";

/** Read the dragged page id from a drop event's data transfer. */
export function draggedPageId(data: Pick<DataTransfer, "getData"> | null): string | null {
  if (data === null) return null;
  const raw = data.getData(PAGE_DRAG_MIME).trim();
  return raw === "" ? null : raw;
}

/**
 * Where a drop resolves to: the `over` page id, or null for the top level.
 * A drop on the container (no row under the pointer) is the top level.
 */
export function dropTargetId(over: string | null | undefined): string | null {
  if (over === undefined) return null;
  return over;
}

export interface PageTreeProps {
  workspaceSlug: string;
  projectId: string;
  /** The visible rows of the active tab, already sorted and filtered. */
  rows: readonly PageRowData[];
  /** Every page the caller has loaded, for move legality. */
  pool: readonly PageRowData[];
  /** The active tab; children outside it never enter the tree (PAGE-085). */
  tab: PageListTab;
  /** The active list sort; each sibling list keeps favourite-first. */
  sort: PageSort;
  /** Children of an archived parent are shelved too (PAGE-088). */
  includeArchivedChildren?: boolean;
  showStar: boolean;
  canDuplicate: boolean;
  currentUserId: string | null;
  viewerIsAdmin: boolean;
  /** Whether the caller may reparent at all (member; rows narrow to owner/admin). */
  canMove: boolean;
  /** The edition serves the cross-project move (PAGE-017), a different dialog. */
  canMoveProject: boolean;
  owners: readonly PageCreatorOption[];
  onCreateSubPage: (page: PageRowData) => void;
  onOpenInNewTab: (page: PageRowData) => void;
  onCopyLink: (page: PageRowData) => void;
  onDuplicate: (page: PageRowData) => void;
  onMovePage: (page: PageRowData, targetId: string | null) => void;
  /** Overrides the row's own archive write so the subtree can be credited. */
  onToggleArchive: (page: PageRowData, archived: boolean, pool: readonly PageRowData[]) => void;
  /** Opens the within-project move picker for this page. */
  onRequestMove: (page: PageRowData) => void;
}

function useTreeChildren(
  workspaceSlug: string,
  projectId: string,
  wanted: readonly string[],
  includeArchived: boolean
) {
  // The options array keeps one identity per wanted set: a fresh object per
  // render makes useQueries resubscribe on every commit.
  const wantedKey = wanted.join(",");
  const queries = React.useMemo(
    () =>
      wantedKey
        .split(",")
        .filter((id) => id !== "")
        .map((pageId) => pageChildrenQueryOptions(getAppClient(), workspaceSlug, projectId, pageId, includeArchived)),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- wantedKey is the identity
    [workspaceSlug, projectId, wantedKey, includeArchived]
  );
  const results = useQueries({ queries });
  const children = new Map<string, readonly PageRowData[]>();
  const pending = new Set<string>();
  wanted.forEach((pageId, index) => {
    const result = results[index]!;
    if (result.data !== undefined) children.set(pageId, result.data);
    if (result.isPending) pending.add(pageId);
  });
  return { children, pending };
}

export function PageTree(props: PageTreeProps): React.ReactElement {
  const {
    workspaceSlug,
    projectId,
    rows,
    pool,
    tab,
    sort,
    includeArchivedChildren = false,
    showStar,
    canDuplicate,
    currentUserId,
    viewerIsAdmin,
    canMove,
    canMoveProject,
    owners,
    onCreateSubPage,
    onOpenInNewTab,
    onCopyLink,
    onDuplicate,
    onMovePage,
    onToggleArchive,
    onRequestMove,
  } = props;
  const expanded = usePageExpansion(workspaceSlug, projectId);
  const [dragOver, setDragOver] = React.useState<string | null | undefined>(undefined);
  // A parent can contribute a read once it is on screen: a root or a child
  // already loaded under another expanded parent. An expansion recorded
  // under a different tab stays dormant until that tab shows the row. The
  // wanted ids are keyed by a string so the query array keeps one identity
  // across renders; a fresh array every render would re-subscribe useQueries
  // forever.
  const rootIds = React.useMemo(() => new Set(rows.map((row) => row.id)), [rows]);
  const [wantedKey, setWantedKey] = React.useState("");
  const wanted = React.useMemo(() => (wantedKey === "" ? [] : wantedKey.split(",")), [wantedKey]);
  const { children, pending } = useTreeChildren(workspaceSlug, projectId, wanted, includeArchivedChildren);
  // Once a parent's children arrive they become eligible to expand too, so
  // the recursion deepens one read at a time without needing the whole tree.
  const visibleIds = React.useMemo(() => {
    const ids = new Set(rootIds);
    for (const list of children.values()) for (const child of list) ids.add(child.id);
    return ids;
  }, [rootIds, children]);
  const nextWantedKey = [...expanded]
    .filter((pageId) => visibleIds.has(pageId))
    .sort()
    .join(",");
  React.useEffect(() => {
    setWantedKey((current) => (current === nextWantedKey ? current : nextWantedKey));
  }, [nextWantedKey]);
  const activeChildren = React.useMemo(() => {
    const active = new Map<string, readonly PageRowData[]>();
    for (const pageId of wanted) {
      const list = children.get(pageId);
      // The backend's `?parent=` branch has no archived filter, so the tab
      // partition is applied here as well as to the top-level rows.
      if (list !== undefined)
        active.set(
          pageId,
          sortPageSiblings(
            list.filter((row) => pageInTab(row, tab)),
            sort
          )
        );
    }
    return active;
  }, [children, wanted, sort, tab]);
  const entries = React.useMemo(
    () => flattenPageTree(rows, { expanded, children: activeChildren, pending }),
    [rows, expanded, activeChildren, pending]
  );
  // A loaded child is not in the top-level collection, so resolve drops
  // from everything on screen plus whatever the caller already held.
  const movePool = React.useMemo(() => {
    const byId = new Map<string, PageRowData>();
    for (const row of pool) byId.set(row.id, row);
    for (const entry of entries) byId.set(entry.page.id, entry.page);
    for (const list of children.values()) for (const row of list) byId.set(row.id, row);
    return [...byId.values()];
  }, [pool, entries, children]);

  const ownerOf = (page: PageRowData): PageCreatorOption | null =>
    owners.find((creator) => creator.id === page.owned_by) ?? null;

  function drop(pageId: string, over: string | null | undefined): void {
    setDragOver(undefined);
    const targetId = dropTargetId(over);
    const page = movePool.find((row) => row.id === pageId);
    if (page === undefined) return;
    const own = viewerIsAdmin || (currentUserId !== null && page.owned_by === currentUserId);
    if (!canMove || !own) return;
    const refusal = pageMoveRefusal(movePool, pageId, targetId);
    if (refusal !== null) {
      notifyError(PAGE_MOVE_REFUSED_TITLE, pageMoveRefusalMessage(refusal));
      return;
    }
    onMovePage(page, targetId);
  }

  return (
    <div
      data-testid="pages-tree"
      onDragOver={(event) => {
        if (!canMove) return;
        event.preventDefault();
        // The container is always the top-level target, including after the
        // pointer has crossed a row: leaving the row's id here would make a
        // background drop nest under the last hovered row (PAGE-087).
        setDragOver(null);
      }}
      onDrop={(event) => {
        if (!canMove) return;
        event.preventDefault();
        const pageId = draggedPageId(event.dataTransfer);
        if (pageId === null) return;
        drop(pageId, dragOver);
      }}
    >
      {entries.map((entry: PageTreeEntry) => {
        const { page, depth } = entry;
        const title = pageDisplayTitle(page.name);
        const manage = viewerIsAdmin || (currentUserId !== null && page.owned_by === currentUserId);
        const rowCanDrag = canMove && manage;
        // `sub_pages_count` counts a page's live children, so a cascaded
        // archived parent reports zero even though it still has a shelved
        // subtree (PAGE-088). The archived tab therefore keeps the expand
        // control until that parent's lazy `?parent=` read settles; once it
        // has, the loaded children decide. Live rows follow the count.
        const archivedUnresolved = tab === "archived" && !activeChildren.has(page.id);
        const expandable = entry.hasChildren || archivedUnresolved;
        return (
          <div
            key={page.id}
            data-testid="page-tree-item"
            data-page-id={page.id}
            data-depth={depth}
            draggable={rowCanDrag}
            onDragStart={
              rowCanDrag
                ? (event) => {
                    event.dataTransfer.setData(PAGE_DRAG_MIME, page.id);
                    event.dataTransfer.effectAllowed = "move";
                  }
                : undefined
            }
            onDragOver={
              canMove
                ? (event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    if (dragOver !== page.id) setDragOver(page.id);
                  }
                : undefined
            }
            onDrop={
              canMove
                ? (event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    const pageId = draggedPageId(event.dataTransfer);
                    if (pageId === null) return;
                    drop(pageId, page.id);
                  }
                : undefined
            }
            className={dragOver === page.id ? "border-t-2 border-(--accent)" : undefined}
            style={{ marginInlineStart: `calc(${depth} * var(--space-4))` }}
          >
            <div className="flex min-w-0 items-start gap-(--space-1)">
              {rowCanDrag ? (
                <span aria-hidden="true" className="mt-(--space-3) inline-flex text-(--text-muted)">
                  <GripVertical size={14} strokeWidth={1.5} />
                </span>
              ) : null}
              {expandable ? (
                <button
                  type="button"
                  aria-label={expandLabel(title, entry.expanded)}
                  aria-expanded={entry.expanded}
                  data-testid="page-tree-expand"
                  className="mt-(--space-3) inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-(--radius-control) text-(--text-muted) hover:bg-(--subtle)"
                  onClick={() => {
                    togglePageExpanded(workspaceSlug, projectId, page.id);
                  }}
                >
                  {entry.loading ? (
                    <Loader2 size={14} strokeWidth={1.5} aria-hidden="true" className="animate-spin" />
                  ) : (
                    <ChevronRight
                      size={14}
                      strokeWidth={1.5}
                      aria-hidden="true"
                      className={entry.expanded ? "rotate-90 transition-transform" : "transition-transform"}
                    />
                  )}
                </button>
              ) : (
                <span aria-hidden="true" className="size-5 shrink-0" />
              )}
              <div className="min-w-0 flex-1">
                <PageRowConnected
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  page={page}
                  owner={ownerOf(page)}
                  showStar={showStar}
                  canDuplicate={canDuplicate}
                  canManage={manage}
                  canMove={canMoveProject}
                  rehomedCount={entry.childCount}
                  onToggleArchive={(archived) => {
                    onToggleArchive(page, archived, movePool);
                  }}
                  onAddSubPage={
                    canDuplicate
                      ? () => {
                          onCreateSubPage(page);
                        }
                      : undefined
                  }
                  onRequestNestedMove={
                    canMove && manage
                      ? () => {
                          onRequestMove(page);
                        }
                      : undefined
                  }
                  onOpenInNewTab={() => {
                    onOpenInNewTab(page);
                  }}
                  onCopyLink={() => {
                    onCopyLink(page);
                  }}
                  onDuplicate={() => {
                    onDuplicate(page);
                  }}
                />
              </div>
              {entry.childCount > 0 ? (
                <span
                  data-testid="page-tree-count"
                  className="text-meta mt-(--space-3) shrink-0 rounded-(--radius-control) bg-(--subtle) px-(--space-1) text-(--text-muted)"
                  aria-label={childCountLabel(entry.childCount)}
                >
                  {entry.childCount}
                </span>
              ) : null}
            </div>
          </div>
        );
      })}
    </div>
  );
}
