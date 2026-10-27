// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Quick menu for one workspace view (parity/specs/views.md §10, VIEW-037).
// The same entries serve a custom tab, a default tab and the detail header:
// Edit and Delete are gated (owner for edit, owner or workspace admin for
// delete) while Open in new tab and Copy link are offered to everyone who
// can see the view. Defaults carry no record, so their hosts pass no
// edit/delete handlers and the pair hides. Copy and new-tab act on the
// absolute detail address through the shared link helper.

import { IconButton, Menu, type MenuEntry } from "@pidash/kit";
import { Ellipsis, ExternalLink, Link2, Pencil, Trash2 } from "lucide-react";
import * as React from "react";

import { platform } from "../../../core/platform/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { copyLinkWithToast } from "../../../shared/shell/shareLink.js";
import { LIST_COPY_MESSAGE } from "../../issues/index.js";

/** What the caller may do to the view behind this menu. */
export interface WorkspaceViewMenuStanding {
  /** The caller owns the view's record. */
  isOwner: boolean;
  /** The caller owns the record or administers the workspace. */
  canDelete: boolean;
}

export interface WorkspaceViewMenuCallbacks {
  /** Null until the edit dialog wires up; the entry hides. */
  onEdit: (() => void) | null;
  onOpenInNewTab: () => void;
  onCopyLink: () => void;
  /** Null until the delete dialog wires up; the entry hides. */
  onDelete: (() => void) | null;
}

/** Entries the standing permits, in menu order. */
export function selectWorkspaceViewMenuEntries(
  standing: WorkspaceViewMenuStanding,
  callbacks: WorkspaceViewMenuCallbacks
): MenuEntry[] {
  const entries: MenuEntry[] = [];
  if (standing.isOwner && callbacks.onEdit !== null) {
    entries.push({
      label: "Edit",
      icon: <Pencil size={16} strokeWidth={1.5} aria-hidden="true" />,
      onSelect: callbacks.onEdit,
    });
  }
  entries.push({
    label: "Open in new tab",
    icon: <ExternalLink size={16} strokeWidth={1.5} aria-hidden="true" />,
    onSelect: callbacks.onOpenInNewTab,
  });
  entries.push({
    label: "Copy link",
    icon: <Link2 size={16} strokeWidth={1.5} aria-hidden="true" />,
    onSelect: callbacks.onCopyLink,
  });
  if (standing.canDelete && callbacks.onDelete !== null) {
    entries.push({
      label: "Delete",
      icon: <Trash2 size={16} strokeWidth={1.5} aria-hidden="true" />,
      onSelect: callbacks.onDelete,
    });
  }
  return entries;
}

export interface WorkspaceViewMenuProps {
  /** Detail path the open/copy entries act on. */
  href: string;
  standing: WorkspaceViewMenuStanding;
  /** Opens the edit dialog; null hides the entry. */
  onEdit: (() => void) | null;
  /** Opens the delete confirmation; null hides the entry. */
  onDelete: (() => void) | null;
  /** Accessible name for the menu trigger. */
  triggerLabel: string;
}

export function WorkspaceViewMenu({
  href,
  standing,
  onEdit,
  onDelete,
  triggerLabel,
}: WorkspaceViewMenuProps): React.ReactElement {
  const entries = selectWorkspaceViewMenuEntries(standing, {
    onEdit,
    onOpenInNewTab: () => void platform.openExternal(platform.shareableUrl(href)),
    // The whole-list ellipsis (ISS-067) shares this menu, so the success
    // body is the issues list wording, not a views-specific sentence.
    onCopyLink: () =>
      void copyLinkWithToast(
        href,
        {
          copied: "Link Copied",
          copiedBody: LIST_COPY_MESSAGE,
          failed: "Couldn't copy the view link.",
        },
        { success: notifySuccess, error: notifyError }
      ),
    onDelete,
  });
  return (
    <Menu
      trigger={
        <IconButton label={triggerLabel} data-testid="workspace-view-menu-trigger">
          <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      }
      items={entries}
    />
  );
}
