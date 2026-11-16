// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page creation rules (parity/specs/pages.md §3.6 and §11.1, PAGE-007/008).
// The quick action and the dialog share one reading of what the server
// receives: a trimmed required title capped at 255 characters, the open or
// restricted flag, and the optional mark. Quick create takes its visibility
// from the tab the operator is looking at; the archived tab has no
// creatable visibility, so it falls back to open.

import { PAGE_ACCESS_PRIVATE, PAGE_ACCESS_PUBLIC, type PageCreateInput, type PageMark } from "@pidash/api-client";

import type { PageListTab } from "./pageList.js";

/** Longest title the create dialog accepts (pinned by the oracle). */
export const PAGE_TITLE_MAX_LENGTH = 255;

/** A quick-created page starts with this name and no mark. */
export const PAGE_QUICK_CREATE_NAME = "Untitled";

/** Inline block shown under the title once it is past the cap. */
export const PAGE_TITLE_TOO_LONG_MESSAGE = `Keep the title to ${PAGE_TITLE_MAX_LENGTH} characters.`;

/** Characters that count toward the cap: surrounding blank space does not. */
export function pageTitleLength(title: string): number {
  return title.trim().length;
}

/** True once the trimmed title is longer than the cap. */
export function pageTitleTooLong(title: string): boolean {
  return pageTitleLength(title) > PAGE_TITLE_MAX_LENGTH;
}

/** True while the trimmed title would post as blank. */
export function pageTitleMissing(title: string): boolean {
  return pageTitleLength(title) === 0;
}

/** Both submit conditions in one place: not blank, not over the cap. */
export function pageDraftSubmittable(title: string): boolean {
  return !pageTitleMissing(title) && !pageTitleTooLong(title);
}

/**
 * Visibility a quick-created page inherits from the active tab: the private
 * tab creates restricted pages and the other two create open ones, since the
 * archive cannot be created into.
 */
export function quickCreateAccess(tab: PageListTab): number {
  return tab === "private" ? PAGE_ACCESS_PRIVATE : PAGE_ACCESS_PUBLIC;
}

/**
 * Narrow the picker's draft logo value to the mark the create route takes.
 * Anything that does not name an emoji or an icon (a cleared draft, an
 * uploaded picture the page form cannot represent) yields no mark.
 */
export function pageMarkFromDraft(draft: Record<string, unknown>): PageMark | undefined {
  const inUse = draft["in_use"];
  if (inUse === "emoji") {
    const emoji = draft["emoji"];
    const value = emoji !== null && typeof emoji === "object" ? (emoji as { value?: unknown }).value : undefined;
    return typeof value === "string" && value !== "" ? { in_use: "emoji", emoji: { value } } : undefined;
  }
  if (inUse === "icon") {
    const icon = draft["icon"];
    if (icon === null || typeof icon !== "object") return undefined;
    const { name, color } = icon as { name?: unknown; color?: unknown };
    if (typeof name !== "string" || name === "") return undefined;
    return {
      in_use: "icon",
      icon: { name, ...(typeof color === "string" && color !== "" ? { color } : {}) },
    };
  }
  return undefined;
}

/** Body the create route receives for a dialog draft or a quick create. */
export function pageCreateInput(title: string, access: number, mark?: PageMark | undefined): PageCreateInput {
  return {
    name: title.trim(),
    access,
    ...(mark === undefined ? {} : { logo_props: mark }),
  };
}
