// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project view detail model (parity/specs/views.md §7, VIEW-021–024).
// Pure: the screen owns reads and navigation; this module decides the tab
// title, the layout a stored record opens with, whether the owner-only
// lock shows, and the copy the missing-view state renders.

import type { SavedView } from "@pidash/api-client";

import { parseLayoutKind, type LayoutKind } from "../../issues/index.js";

/** Heading pinned by the oracle for every unreadable view (VIEW-022). */
export const MISSING_VIEW_TITLE = "View does not exist";

/** Explanation under that heading: absent and refused read the same. */
export const MISSING_VIEW_DESCRIPTION = "This view does not exist, or you do not have permission to open it.";

/** The way back to the project's views list (VIEW-022). */
export const MISSING_VIEW_BACK_LABEL = "View other views";

/**
 * Browser tab title: the project name, a separator, then the view name
 * (VIEW-021). A project that has not resolved yet leaves its half out
 * rather than rendering a leading separator.
 */
export function savedViewTitle(projectName: string | undefined, viewName: string): string {
  return projectName === undefined || projectName === "" ? viewName : `${projectName} - ${viewName}`;
}

/** The layout a record stores; unknown values open the list (VIEW-028). */
export function savedViewLayout(view: Pick<SavedView, "display_filters">): LayoutKind {
  return parseLayoutKind(view.display_filters["layout"]);
}

/**
 * Owner-only views (`access` other than the shared value 1) carry the lock
 * glyph (VIEW-024). Shared records show nothing extra.
 */
export function showsPrivateLock(view: Pick<SavedView, "access">): boolean {
  return view.access !== 1;
}

/** A blank project name never becomes a tab title on its own. */
export function savedViewDocumentTitle(projectName: string | undefined, view: Pick<SavedView, "name"> | null): string {
  if (view === null) return projectName === undefined || projectName === "" ? "Views" : projectName;
  return savedViewTitle(projectName, view.name);
}
