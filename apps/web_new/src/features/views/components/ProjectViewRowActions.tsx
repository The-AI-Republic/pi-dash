// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row actions on a project view row (parity/specs/views.md §6, VIEW-012,
// VIEW-015–018, VIEW-020): the favorite star for members and admins, the
// permission-gated overflow menu, and the edit, delete and publish
// dialogs the menu opens. The server enforces the same ownership rules;
// a refused write surfaces as a notice.

import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { deleteProjectView, type SavedView, type SavedViewList } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { edition } from "../../../core/edition/index.js";
import { platform } from "../../../core/platform/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { copyWorkItemLink } from "../../../shared/shell/shareLink.js";
import { loadRichFilterBuilder } from "../../issues/index.js";
import { isProjectAdmin } from "../../projects/index.js";
import { sidebarKeys } from "../../sidebar/index.js";
import { viewKeys } from "../api/keys.js";
import { useToggleViewFavorite } from "../api/viewFavorites.js";
import { DeleteViewDialog } from "../dialog/DeleteViewDialog.js";
import { UpdateViewDialog, type CreateViewDialogProps } from "../dialog/CreateViewDialog.js";
import { PublishViewDialog } from "../dialog/PublishViewDialog.js";
import {
  canEditProjectView,
  canStarProjectView,
  isViewFavorited,
  removeProjectViewRow,
  selectProjectViewMenuItems,
  setProjectViewAnchor,
  withViewAnchor,
  type ProjectViewMenuItemId,
} from "../lib/projectViewActions.js";
import { isViewLive, projectViewHref } from "../lib/projectViews.js";
import { ProjectViewRowMenu } from "./ProjectViewRowMenu.js";
import { ProjectViewStar } from "./ProjectViewStar.js";

export const VIEW_DELETED_TITLE = "View deleted successfully";
export const VIEW_DELETE_FAILED_TITLE = "View could not be deleted";
export const VIEW_PUBLISHED_TITLE = "View published";
export const VIEW_PUBLISH_FAILED_TITLE = "View could not be published";
export const VIEW_UNPUBLISHED_TITLE = "View unpublished";
export const VIEW_UNPUBLISH_FAILED_TITLE = "View could not be unpublished";

const COPY_VIEW_LINK_MESSAGES = {
  copied: "Link Copied",
  copiedBody: "The view address is on the clipboard.",
  failed: "Couldn't copy the link",
} as const;

export interface ProjectViewRowActionsProps {
  workspaceSlug: string;
  projectId: string;
  view: SavedView;
  /** The signed-in user id, or null while unresolved. */
  viewerId: string | null;
  /** The caller's project role, or null while unresolved. */
  projectRole: number | null;
  /** Cycle/module entries hide while their project features are off. */
  hideCycle: boolean;
  hideModule: boolean;
  /**
   * The publish dialog's open state, owned by the row: the Live marker
   * beside this cluster opens the same dialog as the menu entry.
   */
  publishOpen: boolean;
  onPublishOpenChange: (open: boolean) => void;
}

export function ProjectViewRowActions({
  workspaceSlug,
  projectId,
  view,
  viewerId,
  projectRole,
  hideCycle,
  hideModule,
  publishOpen,
  onPublishOpenChange,
}: ProjectViewRowActionsProps): React.ReactElement {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const favorite = useToggleViewFavorite();
  const [editOpen, setEditOpen] = React.useState(false);
  const [deleteOpen, setDeleteOpen] = React.useState(false);
  const [deleting, setDeleting] = React.useState(false);
  const [publishPending, setPublishPending] = React.useState(false);

  const isOwner = canEditProjectView(viewerId, view);
  const isAdmin = isProjectAdmin(projectRole);
  const publishSlot = edition.slots?.viewPublish;
  const items = selectProjectViewMenuItems({
    isOwner,
    isAdmin,
    canPublish: publishSlot !== undefined,
  });
  const publicLink = publishSlot === undefined || !isViewLive(view.anchor) ? null : publishSlot.publicLink(view.anchor);

  async function runDelete(): Promise<void> {
    setDeleting(true);
    try {
      // The detail key arrives with the builder chunk (size gate); the
      // list screen preloads it, so this reuses the cached request.
      const builder = await loadRichFilterBuilder();
      await deleteProjectView(getAppClient(), workspaceSlug, projectId, view.id);
      queryClient.setQueryData(viewKeys.projectList(workspaceSlug, projectId), (list: SavedViewList | undefined) =>
        removeProjectViewRow(list, view.id)
      );
      queryClient.removeQueries({
        queryKey: builder.savedViewKeys.project(workspaceSlug, projectId, view.id),
      });
      // A deleted view's favorite row disappears with it; the sidebar
      // re-reads so a starred-then-deleted row leaves at once.
      void queryClient.invalidateQueries({ queryKey: sidebarKeys.favorites(workspaceSlug) });
      notifySuccess(VIEW_DELETED_TITLE);
      setDeleteOpen(false);
      await navigate({
        to: "/$ws/projects/$projectId/views",
        params: { ws: workspaceSlug, projectId },
      });
    } catch (error) {
      // The row stays and the dialog stays open for retry (VIEW-016).
      notifyError(VIEW_DELETE_FAILED_TITLE, (error as Error).message);
    } finally {
      setDeleting(false);
    }
  }

  /** Adopt a published or cleared anchor into the cached records. */
  async function adoptAnchor(anchor: string | null): Promise<void> {
    queryClient.setQueryData(viewKeys.projectList(workspaceSlug, projectId), (list: SavedViewList | undefined) =>
      setProjectViewAnchor(list, view.id, anchor)
    );
    const builder = await loadRichFilterBuilder();
    queryClient.setQueryData(
      builder.savedViewKeys.project(workspaceSlug, projectId, view.id),
      (record: SavedView | undefined) => withViewAnchor(record, anchor)
    );
  }

  async function runPublish(): Promise<void> {
    const slot = edition.slots?.viewPublish;
    if (slot === undefined) return;
    setPublishPending(true);
    try {
      const { anchor } = await slot.publish({ workspaceSlug, projectId, viewId: view.id });
      await adoptAnchor(anchor);
      notifySuccess(VIEW_PUBLISHED_TITLE, "The public link is ready to share.");
    } catch (error) {
      notifyError(VIEW_PUBLISH_FAILED_TITLE, (error as Error).message);
    } finally {
      setPublishPending(false);
    }
  }

  async function runUnpublish(): Promise<void> {
    const slot = edition.slots?.viewPublish;
    if (slot === undefined) return;
    setPublishPending(true);
    try {
      await slot.unpublish({ workspaceSlug, projectId, viewId: view.id });
      await adoptAnchor(null);
      notifySuccess(VIEW_UNPUBLISHED_TITLE, "The public link no longer works.");
    } catch (error) {
      notifyError(VIEW_UNPUBLISH_FAILED_TITLE, (error as Error).message);
    } finally {
      setPublishPending(false);
    }
  }

  function select(item: ProjectViewMenuItemId): void {
    switch (item) {
      case "edit":
        setEditOpen(true);
        break;
      case "open":
        void platform.openExternal(platform.shareableUrl(projectViewHref(workspaceSlug, projectId, view.id)));
        break;
      case "copy":
        void copyWorkItemLink(projectViewHref(workspaceSlug, projectId, view.id), COPY_VIEW_LINK_MESSAGES);
        break;
      case "publish":
        onPublishOpenChange(true);
        break;
      case "delete":
        setDeleteOpen(true);
        break;
    }
  }

  const dialogProps: CreateViewDialogProps = {
    open: editOpen,
    onOpenChange: setEditOpen,
    workspaceSlug,
    projectId,
    hideCycle,
    hideModule,
  };
  return (
    <span className="flex shrink-0 items-center gap-(--space-1)">
      {canStarProjectView(projectRole) ? (
        <ProjectViewStar
          viewName={view.name}
          favorited={isViewFavorited(view.is_favorite)}
          pending={favorite.pendingViewId === view.id}
          onToggle={() => {
            favorite.mutate({ workspaceSlug, projectId, view });
          }}
        />
      ) : null}
      <ProjectViewRowMenu viewName={view.name} items={items} onSelect={select} />
      <UpdateViewDialog {...dialogProps} view={view} />
      <DeleteViewDialog
        open={deleteOpen}
        deleting={deleting}
        onCancel={() => {
          setDeleteOpen(false);
        }}
        onConfirm={() => {
          void runDelete();
        }}
      />
      <PublishViewDialog
        open={publishOpen}
        viewName={view.name}
        link={publicLink}
        pending={publishPending}
        onPublish={() => {
          void runPublish();
        }}
        onUnpublish={() => {
          void runUnpublish();
        }}
        onCopyLink={(link) => {
          void copyWorkItemLink(link, COPY_VIEW_LINK_MESSAGES);
        }}
        onClose={() => {
          onPublishOpenChange(false);
        }}
      />
    </span>
  );
}
