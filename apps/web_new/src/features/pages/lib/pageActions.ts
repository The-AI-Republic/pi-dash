// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page row actions (parity/specs/pages.md §3.8, PAGE-010–012). One builder
// feeds both the overflow button and the right-click menu, in the pinned
// owner order and gated by capability. The viewer-safe pair leads the menu
// for every viewer; duplication follows for members and admins. Later
// slices append their owner-only entries here instead of forking a second
// menu, so the order stays pinned in one place.

import type { MenuAction } from "@pidash/kit";

/** Labels pinned by the oracle. */
export const PAGE_ACTION_OPEN_IN_NEW_TAB = "Open in new tab";
export const PAGE_ACTION_COPY_LINK = "Copy link";
export const PAGE_ACTION_DUPLICATE = "Make a copy";

/** Outcome notices for the two row writes this slice owns. */
export const PAGE_DUPLICATED_TITLE = "Page duplicated successfully.";
export const PAGE_DUPLICATE_FAILED_TITLE = "Page could not be duplicated.";

export interface PageRowCapabilities {
  /** Members and admins may copy a page; every viewer may link to it. */
  duplicate: boolean;
}

export interface PageRowHandlers {
  openInNewTab: () => void;
  copyLink: () => void;
  duplicate: () => void;
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
  return entries;
}
