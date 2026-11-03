// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail options menu (parity/specs/pages.md §7.4, PAGE-060) and the
// shelved header marker (spec §2.3, PAGE-059). One builder feeds the
// header's options popup in the pinned owner order, gated per PAGE-018:
// the display toggles, the hosted markup copy (PAGE-044, editors only),
// the history shortcut, the sub-page entry, duplication for members and
// above, shelve/restore, visibility and delete for the owner or an admin,
// and the hosted export entry (PAGE-074) once someone wires it. The list
// row menu owns new-tab, copy-link, lock and move — they never appear
// here. The marker renders the shelved stamp as a dated header line.

import type { MenuAction } from "@pidash/kit";

import type { PageBodyOptionActions } from "./pageBodyActions.js";
import { formatCustomDate } from "./pageList.js";
import {
  PAGE_ACTION_ARCHIVE,
  PAGE_ACTION_DELETE,
  PAGE_ACTION_DUPLICATE,
  PAGE_ACTION_MAKE_PRIVATE,
  PAGE_ACTION_MAKE_PUBLIC,
  PAGE_ACTION_RESTORE,
} from "./pageActions.js";
import { pageDisplayMenuItems } from "./pageDisplayMenu.js";
import type { PagePreferences } from "./pagePreferences.js";
import { PAGE_ACTION_ADD_SUB_PAGE } from "./pageNested.js";

/** Labels this menu owns; the rest are shared with the row menu. */
export const PAGE_HISTORY_ITEM = "Version history";
export const PAGE_EXPORT_ITEM = "Export";

export interface PageDetailMenuCapabilities {
  /** Members and admins may copy a page. */
  canDuplicate: boolean;
  /** The page's owner or a project admin may shelve, switch visibility, delete. */
  canManage: boolean;
  /** Shelved pages carry Restore and Delete; live pages carry Archive. */
  archived: boolean;
  /** The record's current access chooses Make public or Make private. */
  restricted: boolean;
  /** Live editor actions; null on a read-only or unloaded page (PAGE-044). */
  body: PageBodyOptionActions | null;
}

export interface PageDetailMenuHandlers {
  onOpenHistory: () => void;
  onDuplicate: () => void;
  onToggleArchive: () => void;
  onToggleVisibility: () => void;
  onRequestDelete: () => void;
  /** Opens the registered export dialog; absent until the exports area mounts it (PAGE-074). */
  onExport?: (() => void) | undefined;
  /** Creates a child page; absent for guests and on shelved pages (PAGE-084). */
  onAddSubPage?: (() => void) | undefined;
}

/** Menu entries for the detail header, in the order the oracle pins them. */
export function pageDetailMenuItems(
  capabilities: PageDetailMenuCapabilities,
  handlers: PageDetailMenuHandlers,
  preferences: PagePreferences,
  onPreferenceChange: (patch: Partial<PagePreferences>) => void
): MenuAction[] {
  const entries: MenuAction[] = pageDisplayMenuItems(preferences, onPreferenceChange, {
    hideStickyToolbar: capabilities.archived,
  });
  if (capabilities.body !== null) {
    const body = capabilities.body;
    entries.push({ label: body.copyMarkdownLabel, onSelect: () => void body.copyMarkdown() });
  }
  entries.push({ label: PAGE_HISTORY_ITEM, onSelect: handlers.onOpenHistory });
  if (handlers.onAddSubPage !== undefined) {
    entries.push({ label: PAGE_ACTION_ADD_SUB_PAGE, onSelect: handlers.onAddSubPage });
  }
  if (capabilities.canDuplicate) {
    entries.push({ label: PAGE_ACTION_DUPLICATE, onSelect: handlers.onDuplicate });
  }
  if (capabilities.canManage) {
    entries.push({
      label: capabilities.archived ? PAGE_ACTION_RESTORE : PAGE_ACTION_ARCHIVE,
      onSelect: handlers.onToggleArchive,
    });
    if (!capabilities.archived) {
      entries.push({
        label: capabilities.restricted ? PAGE_ACTION_MAKE_PUBLIC : PAGE_ACTION_MAKE_PRIVATE,
        onSelect: handlers.onToggleVisibility,
      });
    } else {
      entries.push({ label: PAGE_ACTION_DELETE, onSelect: handlers.onRequestDelete });
    }
  }
  if (handlers.onExport !== undefined) {
    entries.push({ label: PAGE_EXPORT_ITEM, onSelect: handlers.onExport });
  }
  return entries;
}

/** The shelved header line for a date-only archive stamp (PAGE-059). */
export function pageArchivedMarker(archivedAt: string): string {
  return `Archived at ${formatCustomDate(archivedAt)}`;
}
