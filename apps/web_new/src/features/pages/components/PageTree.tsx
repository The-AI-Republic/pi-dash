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
import { pageChildrenQueryOptions } from "../api/treeQueries.js";
import type { PageCreatorOption } from "../lib/pageList.js";
import { flattenPageTree, pageChildCount, type PageTreeEntry } from "../lib/pageNested.js";
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
  onToggleArchive: (page: PageRowData, archived: boolean) => void;
  /** Opens the within-project move picker for this page. */
  onRequestMove: (page: PageRowData) => void;
}

function useTreeChildren(workspaceSlug: string, projectId: string, wanted: readonly string[]) {
  // The options array keeps one identity per wanted set: a fresh object per
  // render makes useQueries resubscribe on every commit.
  const wantedKey = wanted.join(",");
  const queries = React.useMemo(
    () =>
      wantedKey
        .split(",")
        .filter((id) => id !== "")
        .map((pageId) => pageChildrenQueryOptions(getAppClient(), workspaceSlug, projectId, pageId)),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- wantedKey is the identity
    [workspaceSlug, projectId, wantedKey]
  );
  const results = useQueries({ queries });
  const children = new Map<string, readonly PageRowData[]>();
  const pending = new Set<string>();
  wanted.forEach((pageId, index) => {
    const result = results[index];
    if (result === undefined) return;
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
  const { children, pending } = useTreeChildren(workspaceSlug, projectId, wanted);
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
      if (list !== undefined) active.set(pageId, list);
    }
    return active;
  }, [children, wanted]);
  const entries = React.useMemo(
    () => flattenPageTree(rows, { expanded, children: activeChildren, pending }),
    [rows, expanded, activeChildren, pending]
  );

  const ownerOf = (page: PageRowData): PageCreatorOption | null =>
    owners.find((creator) => creator.id === page.owned_by) ?? null;

  function drop(pageId: string, over: string | null | undefined): void {
    setDragOver(undefined);
    const targetId = dropTargetId(over);
    if (targetId === pageId) return;
    const page = pool.find((row) => row.id === pageId);
    if (page === undefined) return;
    onMovePage(page, targetId);
  }

  return (
    <div
      data-testid="pages-tree"
      onDragOver={(event) => {
        if (!canMove) return;
        event.preventDefault();
        if (dragOver === undefined) setDragOver(null);
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
        const rowCanMove = canMove && manage;
        return (
          <div
            key={page.id}
            data-testid="page-tree-item"
            data-page-id={page.id}
            data-depth={depth}
            draggable={rowCanMove}
            onDragStart={
              rowCanMove
                ? (event) => {
                    event.dataTransfer.setData(PAGE_DRAG_MIME, page.id);
                    event.dataTransfer.effectAllowed = "move";
                  }
                : undefined
            }
            onDragOver={
              rowCanMove
                ? (event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    if (dragOver !== page.id) setDragOver(page.id);
                  }
                : undefined
            }
            onDrop={
              rowCanMove
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
              {rowCanMove ? (
                <span aria-hidden="true" className="mt-(--space-3) inline-flex text-(--text-muted)">
                  <GripVertical size={14} strokeWidth={1.5} />
                </span>
              ) : null}
              {entry.hasChildren ? (
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
                  rehomedCount={pageChildCount(page)}
                  onToggleArchive={(archived) => {
                    onToggleArchive(page, archived);
                  }}
                  onAddSubPage={
                    canDuplicate
                      ? () => {
                          onCreateSubPage(page);
                        }
                      : undefined
                  }
                  onRequestNestedMove={
                    rowCanMove
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
              {pageChildCount(page) > 0 ? (
                <span
                  data-testid="page-tree-count"
                  className="text-meta mt-(--space-3) shrink-0 rounded-(--radius-control) bg-(--subtle) px-(--space-1) text-(--text-muted)"
                  aria-label={childCountLabel(pageChildCount(page))}
                >
                  {pageChildCount(page)}
                </span>
              ) : null}
            </div>
          </div>
        );
      })}
    </div>
  );
}
