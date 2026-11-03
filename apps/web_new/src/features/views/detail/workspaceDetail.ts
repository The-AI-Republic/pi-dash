// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace view detail model (parity/specs/views.md §11, VIEW-038/040).
// Pure: the screen owns reads and navigation; this module resolves the
// route id to a static default or a visible custom, builds the workspace
// issue-read inputs (a default's static condition or a custom's stored
// expression), and decides which stored layout the body renders.

import type { SavedView, SavedViewPatch } from "@pidash/api-client";

import type { Edition } from "../../../core/edition/index.js";
import { isLayoutKind, type LayoutKind } from "../../issues/index.js";
import { WORKSPACE_DEFAULT_VIEWS, type WorkspaceDefaultView } from "../lib/workspaceViews.js";

/** A default's definition, or null when the id names a custom (or nothing). */
export function findWorkspaceDefaultView(viewId: string): WorkspaceDefaultView | null {
  return WORKSPACE_DEFAULT_VIEWS.find((view) => view.key === viewId) ?? null;
}

/**
 * Membership of the id in the caller's visible custom list. The list read
 * is the access check (spec §11): the retrieve alone is not
 * guest-narrowed, so a custom the list hides never renders, however the
 * retrieve answers.
 */
export function findVisibleCustomView(views: readonly SavedView[], viewId: string): SavedView | null {
  return views.find((view) => view.id === viewId) ?? null;
}

/**
 * A default's static issue condition for the current user (spec §11): the
 * assigned view keeps their items, created keeps what they opened,
 * subscribed keeps what they follow, and all-issues adds no condition.
 */
export function workspaceDefaultStaticFilters(view: WorkspaceDefaultView, meId: string): Record<string, string> | null {
  switch (view.key) {
    case "assigned":
      return { assignees: meId };
    case "created":
      return { created_by: meId };
    case "subscribed":
      return { subscriber: meId };
    default:
      return null;
  }
}

/**
 * Layouts the edition offers on the workspace surface, in selector order
 * (VIEW-040). Unknown keys are dropped: the slot trades in server layout
 * keys and the app only renders keys it knows. OSS leaves the slot absent
 * and offers nothing — no selector renders.
 */
export function offeredWorkspaceLayouts(edition: Edition): LayoutKind[] {
  const layouts = edition.slots?.workspaceViewLayouts?.layouts ?? [];
  return layouts.filter(isLayoutKind);
}

/**
 * Whether the body renders rows for the stored layout, and which layout
 * it renders as (VIEW-040). An absent value opens the spreadsheet, the
 * surface's only OSS layout; a cloud-offered layout renders through the
 * surface renderer; anything else renders nothing, matching the old OSS
 * stub for customs whose display payload names an undrawable layout.
 */
export function resolveWorkspaceBodyLayout(stored: unknown, offered: readonly LayoutKind[]): LayoutKind | null {
  if (stored === undefined || stored === null) return "spreadsheet";
  if (stored === "spreadsheet") return "spreadsheet";
  if (isLayoutKind(stored) && offered.includes(stored)) return stored;
  return null;
}

/**
 * The layout the cloud selector marks active: the stored value when it
 * names a layout, else the spreadsheet the body opens with. A stored
 * layout the edition does not offer marks nothing — the record keeps a
 * choice the surface cannot draw.
 */
export function workspaceActiveLayout(view: Pick<SavedView, "display_filters">): LayoutKind {
  const stored = view.display_filters["layout"];
  return isLayoutKind(stored) ? stored : "spreadsheet";
}

/** A layout switch carried through the view's display-filter update (VIEW-040). */
export function workspaceLayoutPatch(view: Pick<SavedView, "display_filters">, layout: LayoutKind): SavedViewPatch {
  return { display_filters: { ...view.display_filters, layout } };
}
