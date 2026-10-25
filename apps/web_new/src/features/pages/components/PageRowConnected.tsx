// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The interactive row: it owns this page's state writes and the two dialogs
// those writes can open (parity/specs/pages.md §3.8, PAGE-013–017), and
// hands the presentation row its handlers. One row mount therefore holds
// exactly one delete dialog and one move dialog, each only while open, and
// a write in flight pins its own row's action control through the hook's
// pending flag.

import type { PageRow as PageRowData } from "@pidash/api-client";
import * as React from "react";

import {
  usePageArchiveWrite,
  usePageDeleteWrite,
  usePageLockWrite,
  usePageMoveWrite,
  usePageVisibilityWrite,
} from "../api/rowMutations.js";
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
  /** Direct children that move up when this page is deleted (PAGE-089). */
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
  rehomedCount = 0,
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
        rehomedCount={rehomedCount}
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
