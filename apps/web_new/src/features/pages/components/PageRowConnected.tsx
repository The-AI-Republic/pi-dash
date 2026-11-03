// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The interactive row: it owns this page's state writes and the two dialogs
// those writes can open (parity/specs/pages.md §3.8, PAGE-013–017), and
// hands the presentation row its handlers. One row mount therefore holds
// exactly one delete dialog and one move dialog, each only while open, and
// a write in flight pins its own row's action control through the hook's
// pending flag.

import { useQuery, queryOptions } from "@tanstack/react-query";
import { listPages, type PageRow as PageRowData } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";

import {
  usePageArchiveWrite,
  usePageDeleteWrite,
  usePageLockWrite,
  usePageMoveWrite,
  usePageVisibilityWrite,
} from "../api/rowMutations.js";
import { pageKeys } from "../api/keys.js";
import type { PageCreatorOption } from "../lib/pageList.js";
import { PageDeleteDialog } from "./PageDeleteDialog.js";
import { PageRow } from "./PageRow.js";
import { PageMoveDialog } from "./PageMoveDialog.js";

export interface PageRowConnectedProps {
  workspaceSlug: string;
  projectId: string;
  page: PageRowData;
  owner: PageCreatorOption | null;
  showStar: boolean;
  canDuplicate: boolean;
  /** The caller owns the page or is a project admin. */
  canManage: boolean;
  /** The edition serves the cross-project move. */
  canMove: boolean;
  /** Creates a child page from this row (PAGE-084); absent for guests. */
  onAddSubPage?: (() => void) | undefined;
  /** Opens the within-project move picker (PAGE-087); owner or admin only. */
  onRequestNestedMove?: (() => void) | undefined;
  /** Ancestor trail as a secondary line (PAGE-090). */
  parentPath?: string | null | undefined;
  /**
   * Direct children that move up when this page is deleted (PAGE-089).
   * Undefined means the caller cannot know from its own reads: an archived
   * parent reports zero live children, so the count is resolved from one
   * `?parent=` read when the dialog opens, without requiring an expansion.
   */
  rehomedCount?: number | undefined;
  /**
   * Overrides the archive write: the nested tree archives whole subtrees and
   * needs to choose the restore notice from the caller's own collection
   * (PAGE-088). Without it the row keeps its own write.
   */
  onToggleArchive?: ((archived: boolean) => void) | undefined;
  onOpenInNewTab: () => void;
  onCopyLink: () => void;
  onDuplicate: () => void;
}

export function PageRowConnected({
  workspaceSlug,
  projectId,
  page,
  owner,
  showStar,
  canDuplicate,
  canManage,
  canMove,
  onAddSubPage,
  onRequestNestedMove,
  parentPath = null,
  rehomedCount,
  onToggleArchive,
  onOpenInNewTab,
  onCopyLink,
  onDuplicate,
}: PageRowConnectedProps): React.ReactElement {
  const target = { workspaceSlug, projectId, page };
  const visibility = usePageVisibilityWrite(target);
  const lock = usePageLockWrite(target);
  const archive = usePageArchiveWrite(target);
  const deletion = usePageDeleteWrite(target);
  const move = usePageMoveWrite(target);
  const [deleteOpen, setDeleteOpen] = React.useState(false);
  const [moveOpen, setMoveOpen] = React.useState(false);
  // A collapsed archived parent cannot report its shelved children through
  // `sub_pages_count`, so the confirmation reads them once the dialog is
  // actually open. Live rows and already-expanded archived rows pass a
  // number and never trigger this read.
  const directChildren = useQuery(
    queryOptions({
      queryKey: pageKeys.children(workspaceSlug, projectId, page.id, true),
      // Resolve the client inside the query so a never-run read (every row
      // whose count is already known) does not need one.
      queryFn: () => listPages(getAppClient(), workspaceSlug, projectId, { parent: page.id, includeArchived: true }),
      staleTime: LIST_STALE_TIME_MS,
      enabled: deleteOpen && rehomedCount === undefined,
    })
  );
  const deleteCount = rehomedCount ?? directChildren.data?.length;

  return (
    <>
      <PageRow
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        page={page}
        owner={owner}
        showStar={showStar}
        canDuplicate={canDuplicate}
        canManage={canManage}
        canMove={canMove}
        parentPath={parentPath}
        {...(onAddSubPage === undefined ? {} : { onAddSubPage })}
        {...(onRequestNestedMove === undefined ? {} : { onRequestNestedMove })}
        onOpenInNewTab={onOpenInNewTab}
        onCopyLink={onCopyLink}
        onDuplicate={onDuplicate}
        onToggleVisibility={() => visibility.setVisibility(page.access !== 1)}
        onToggleLock={() => lock.toggleLock(!page.is_locked)}
        onToggleArchive={() => {
          if (onToggleArchive === undefined) {
            archive.toggleArchive(page.archived_at === null);
            return;
          }
          onToggleArchive(page.archived_at === null);
        }}
        onRequestDelete={() => setDeleteOpen(true)}
        onRequestMove={() => setMoveOpen(true)}
      />
      <PageDeleteDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        pageName={page.name}
        {...(deleteCount === undefined ? {} : { rehomedCount: deleteCount })}
        pending={deletion.pending}
        onConfirm={() => {
          deletion.remove({
            onDeleted: () => setDeleteOpen(false),
          });
        }}
      />
      <PageMoveDialog
        open={moveOpen}
        onOpenChange={setMoveOpen}
        workspaceSlug={workspaceSlug}
        currentProjectId={projectId}
        pageName={page.name}
        pending={move.pending}
        onConfirm={(newProjectId) => {
          move.move(newProjectId);
          setMoveOpen(false);
        }}
      />
    </>
  );
}
