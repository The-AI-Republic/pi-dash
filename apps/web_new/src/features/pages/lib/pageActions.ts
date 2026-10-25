// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page row actions (parity/specs/pages.md §3.8, PAGE-010–017). One builder
// feeds both the overflow button and the right-click menu, in the pinned
// owner order and gated by capability. The viewer-safe pair leads the menu
// for every viewer; duplication follows for members and admins; the state
// switches (lock, visibility, shelve) and the destructive entries close the
// menu for the page's owner or a project admin. Visibility is absent while
// the page is shelved; delete appears only on a shelved page; the
// cross-project move is cloud-only. Everything later slices add goes here
// instead of forking a second menu, so the order stays pinned in one place.

import type { MenuAction } from "@pidash/kit";

/** Labels pinned by the oracle. */
export const PAGE_ACTION_OPEN_IN_NEW_TAB = "Open in new tab";
export const PAGE_ACTION_COPY_LINK = "Copy link";
export const PAGE_ACTION_DUPLICATE = "Make a copy";
export const PAGE_ACTION_LOCK = "Lock";
export const PAGE_ACTION_UNLOCK = "Unlock";
export const PAGE_ACTION_MAKE_PRIVATE = "Make private";
export const PAGE_ACTION_MAKE_PUBLIC = "Make public";
export const PAGE_ACTION_ARCHIVE = "Archive";
export const PAGE_ACTION_RESTORE = "Restore";
export const PAGE_ACTION_DELETE = "Delete";
export const PAGE_ACTION_MOVE = "Move to project…";

/** Outcome notices for the two row writes the earlier slice owns. */
export const PAGE_DUPLICATED_TITLE = "Page duplicated successfully.";
export const PAGE_DUPLICATE_FAILED_TITLE = "Page could not be duplicated.";

/** Outcome notices for the state switches this slice owns. */
export const PAGE_LOCKED_TITLE = "Page locked successfully.";
export const PAGE_UNLOCKED_TITLE = "Page unlocked successfully.";
export const PAGE_LOCK_FAILED_TITLE = "Page could not be locked.";
export const PAGE_ARCHIVED_TITLE = "Page archived successfully.";
export const PAGE_RESTORED_TITLE = "Page restored successfully.";
export const PAGE_ARCHIVE_FAILED_TITLE = "Page could not be archived.";
export const PAGE_RESTORE_FAILED_TITLE = "Page could not be restored.";
export const PAGE_DELETED_TITLE = "Page deleted successfully.";
export const PAGE_DELETE_FAILED_TITLE = "Page could not be deleted.";
export const PAGE_MOVED_TITLE = "Page moved to the new project.";
export const PAGE_MOVE_FAILED_TITLE = "Page could not be moved.";
/** The visibility notice carries this suffix; the oracle matches it. */
export const PAGE_VISIBILITY_SUFFIX = "marked private";
export const PAGE_VISIBILITY_PUBLIC_SUFFIX = "marked public";
export const PAGE_VISIBILITY_FAILED_TITLE = "Page visibility could not be changed.";

export interface PageRowCapabilities {
  /** Members and admins may copy a page; every viewer may link to it. */
  duplicate: boolean;
  /** The page's owner or a project admin may switch lock/visibility/archive. */
  manage: boolean;
  /** Shelved pages carry Restore and Delete; live pages carry Archive. */
  archived: boolean;
  /** The record's current lock state chooses Lock or Unlock. */
  locked: boolean;
  /** The record's current access chooses Make public or Make private. */
  restricted: boolean;
  /** Cross-project move is an edition flag; community builds carry none. */
  move: boolean;
}

export interface PageRowHandlers {
  openInNewTab: () => void;
  copyLink: () => void;
  duplicate: () => void;
  toggleVisibility: () => void;
  toggleLock: () => void;
  toggleArchive: () => void;
  requestDelete: () => void;
  requestMove: () => void;
}

/** Menu entries for one row, in the order the oracle pins them. */
export function pageRowActions(capabilities: PageRowCapabilities, handlers: PageRowHandlers): MenuAction[] {
  const entries: MenuAction[] = [
    { label: PAGE_ACTION_OPEN_IN_NEW_TAB, onSelect: handlers.openInNewTab },
    { label: PAGE_ACTION_COPY_LINK, onSelect: handlers.copyLink },
  ];
  if (capabilities.duplicate) {
    entries.push({ label: PAGE_ACTION_DUPLICATE, onSelect: handlers.duplicate });
  }
  if (capabilities.manage) {
    entries.push({
      label: capabilities.locked ? PAGE_ACTION_UNLOCK : PAGE_ACTION_LOCK,
      onSelect: handlers.toggleLock,
    });
    if (!capabilities.archived) {
      entries.push({
        label: capabilities.restricted ? PAGE_ACTION_MAKE_PUBLIC : PAGE_ACTION_MAKE_PRIVATE,
        onSelect: handlers.toggleVisibility,
      });
    }
    entries.push({
      label: capabilities.archived ? PAGE_ACTION_RESTORE : PAGE_ACTION_ARCHIVE,
      onSelect: handlers.toggleArchive,
    });
    if (capabilities.archived) {
      entries.push({ label: PAGE_ACTION_DELETE, onSelect: handlers.requestDelete });
    }
    if (capabilities.move) {
      entries.push({ label: PAGE_ACTION_MOVE, onSelect: handlers.requestMove });
    }
  }
  return entries;
}
