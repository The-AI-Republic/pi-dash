// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project views list model (parity/specs/views.md §3, VIEW-001/008-011).
// Pure: the screen owns the queries, this module owns the state choice,
// the default order, the tab title, the row hrefs and the owner lookup.

import type { SavedView, WorkspaceMember } from "@pidash/api-client";

/** Breadcrumb label for the project views tab (VIEW-001 pins it). */
export const PROJECT_VIEWS_CRUMB = "Views";

/** `access` value the list badges Public: shared with the project. */
export const VIEW_ACCESS_SHARED = 1;

/** Tab title for the views list: the project name followed by Views. */
export function projectViewsTitle(projectName: string): string {
  return `${projectName} - ${PROJECT_VIEWS_CRUMB}`;
}

/**
 * Default row order: `updated_at` descending (spec §3). The server
 * answers favorites-first-then-name; the screen re-sorts on top, so the
 * presentation never depends on the server order. Stable: ties keep the
 * fetched order. Never mutates the input.
 */
export function sortProjectViewsDefault(views: readonly SavedView[]): SavedView[] {
  // ISO-8601 UTC stamps order lexicographically, so a string comparison
  // is chronological; newest first. Sort is stable: ties keep the fetched
  // order.
  return [...views].sort((left, right) => right.updated_at.localeCompare(left.updated_at));
}

/** Deep link to one saved view (VIEW-011 pins the slash-less shape). */
export function projectViewHref(workspaceSlug: string, projectId: string, viewId: string): string {
  return `/${workspaceSlug}/projects/${projectId}/views/${viewId}`;
}

/**
 * Manage-features shortcut target (VIEW-002): the project's views
 * feature leaf in settings, which renders the views switch (PSET-043).
 * The bare `…/features` index has no page (bug NEWFRONT-247), so the
 * shortcut never points there.
 */
export function projectViewsFeatureHref(workspaceSlug: string, projectId: string): string {
  return `/${workspaceSlug}/settings/projects/${projectId}/features/views`;
}

/**
 * The list body renders exactly one state (VIEW-008/009/010). `viewCount`
 * is the fetched list length, `visibleCount` what the client-side search
 * and filters leave (NEWFRONT-720 owns that set; until it lands the
 * screen passes the full list). The zero-views state wins over no-match
 * when the project has no views at all, even with a search typed.
 */
export type ProjectViewsBodyState = "loading" | "error" | "zero" | "no-match" | "rows";

export interface ProjectViewsBodyInput {
  viewsPending: boolean;
  viewsError: boolean;
  viewCount: number;
  visibleCount: number;
}

export function resolveProjectViewsBody({
  viewsPending,
  viewsError,
  viewCount,
  visibleCount,
}: ProjectViewsBodyInput): ProjectViewsBodyState {
  if (viewsPending) return "loading";
  if (viewsError) return "error";
  if (viewCount === 0) return "zero";
  if (visibleCount === 0) return "no-match";
  return "rows";
}

/** True for shared views (`access = 1`); every other value reads owner-only. */
export function isSharedView(access: number): boolean {
  return access === VIEW_ACCESS_SHARED;
}

/** The row's access badge label (VIEW-011 pins both literals). */
export function viewAccessLabel(access: number): "Public" | "Private" {
  return isSharedView(access) ? "Public" : "Private";
}

/**
 * The Live marker shows only when the cloud publish flow wrote an
 * anchor (VIEW-011). OSS records carry none, so OSS rows never mark.
 * A predicate: a live anchor is always a usable non-empty string.
 */
export function isViewLive(anchor: SavedView["anchor"]): anchor is string {
  return anchor !== undefined && anchor !== null && anchor !== "";
}

/** The owner chip a row renders: display name plus avatar URL. */
export interface ViewOwner {
  name: string;
  avatarUrl: string | null;
}

/**
 * Resolve a view's owner against the workspace roster by user id. Null
 * when the roster is missing or the owner left: the row then shows no
 * avatar rather than a fabricated identity.
 */
export function findViewOwner(members: readonly WorkspaceMember[] | undefined, ownedBy: string): ViewOwner | null {
  const member = members?.find((entry) => entry.member.id === ownedBy)?.member ?? null;
  if (member === null) return null;
  return { name: member.display_name, avatarUrl: member.avatar_url };
}
