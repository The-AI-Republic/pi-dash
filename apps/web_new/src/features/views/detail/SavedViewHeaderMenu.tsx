// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overflow menu on the project view detail header (parity/specs/views.md §7,
// VIEW-021; §6, VIEW-017/020). The header right side carries the layout
// switcher, Display control, filter toggle, Add work item button and this
// menu (desktop). The menu offers edit, open, copy canonical link, cloud
// publish and delete, gated per role: edit for the owner; open and copy
// for everyone who can see the view; publish and delete for the owner or
// a project admin. Copy and open act on the canonical view address
// `/{ws}/projects/{p}/views/{id}`, never the current pathname+search.

import { IconButton, Menu, type MenuEntry } from "@pidash/kit";
import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { deleteProjectView, type SavedView, type SavedViewList } from "@pidash/api-client";
import { Ellipsis, ExternalLink, Globe, Link2, Pencil, Trash2 } from "lucide-react";
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
import { DeleteViewDialog } from "../dialog/DeleteViewDialog.js";
import { UpdateViewDialog, type CreateViewDialogProps } from "../dialog/CreateViewDialog.js";
import { PublishViewDialog } from "../dialog/PublishViewDialog.js";
import {
  canEditProjectView,
  removeProjectViewRow,
  selectProjectViewMenuItems,
  setProjectViewAnchor,
  withViewAnchor,
  type ProjectViewMenuItemId,
} from "../lib/projectViewActions.js";
import { isViewLive, projectViewHref } from "../lib/projectViews.js";

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

const MENU_COPY: Record<ProjectViewMenuItemId, { label: string; icon: React.ReactNode }> = {
  edit: { label: "Edit", icon: <Pencil size={16} strokeWidth={1.5} aria-hidden="true" /> },
  open: { label: "Open in new tab", icon: <ExternalLink size={16} strokeWidth={1.5} aria-hidden="true" /> },
  copy: { label: "Copy link", icon: <Link2 size={16} strokeWidth={1.5} aria-hidden="true" /> },
  publish: { label: "Publish", icon: <Globe size={16} strokeWidth={1.5} aria-hidden="true" /> },
  delete: { label: "Delete", icon: <Trash2 size={16} strokeWidth={1.5} aria-hidden="true" /> },
};

export interface SavedViewHeaderMenuProps {
  workspaceSlug: string;
  projectId: string;
  view: SavedView;
  /** The signed-in user id, or null while unresolved. */
  viewerId: string | null;
  /** The caller's project role, or null while unresolved. */
  projectRole: number | null;
  hideCycle: boolean;
  hideModule: boolean;
}

export function SavedViewHeaderMenu({
  workspaceSlug,
  projectId,
  view,
  viewerId,
  projectRole,
  hideCycle,
  hideModule,
}: SavedViewHeaderMenuProps): React.ReactElement {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [editOpen, setEditOpen] = React.useState(false);
  const [deleteOpen, setDeleteOpen] = React.useState(false);
  const [deleting, setDeleting] = React.useState(false);
  const [publishOpen, setPublishOpen] = React.useState(false);
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
      const builder = await loadRichFilterBuilder();
      await deleteProjectView(getAppClient(), workspaceSlug, projectId, view.id);
      queryClient.setQueryData(viewKeys.projectList(workspaceSlug, projectId), (list: SavedViewList | undefined) =>
        removeProjectViewRow(list, view.id)
      );
      queryClient.removeQueries({
        queryKey: builder.savedViewKeys.project(workspaceSlug, projectId, view.id),
      });
      void queryClient.invalidateQueries({ queryKey: sidebarKeys.favorites(workspaceSlug) });
      notifySuccess(VIEW_DELETED_TITLE);
      setDeleteOpen(false);
      await navigate({
        to: "/$ws/projects/$projectId/views",
        params: { ws: workspaceSlug, projectId },
      });
    } catch (error) {
      notifyError(VIEW_DELETE_FAILED_TITLE, (error as Error).message);
    } finally {
      setDeleting(false);
    }
  }

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
        setPublishOpen(true);
        break;
      case "delete":
        setDeleteOpen(true);
        break;
    }
  }

  const entries: MenuEntry[] = items.map((id) => ({
    label: MENU_COPY[id].label,
    itemKey: id,
    icon: MENU_COPY[id].icon,
    onSelect: () => select(id),
  }));

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
      <Menu
        trigger={
          <IconButton label={`Actions for ${view.name}`} size="small" data-testid="project-view-detail-menu-trigger">
            <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        }
        items={entries}
      />
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
          setPublishOpen(false);
        }}
      />
    </span>
  );
}
