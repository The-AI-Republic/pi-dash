// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile-settings address model (parity/specs/profile.md §5, PROF-009/010/
// 026). Route guards, legacy forwards and the registry need only the tab
// ids and the link shape; the sidebar labels and group names live in
// tabs.ts. Keeping the two apart keeps the label table out of the entry
// chunk, where the route definitions run.

/** The tab every settings entry starts on. */
export const DEFAULT_PROFILE_SETTINGS_TAB = "general" as const;

export type ProfileSettingsTabId =
  | "general"
  | "preferences"
  | "ai-assistant"
  | "auto-project-management"
  | "notifications"
  | "security"
  | "activity"
  | "integrations"
  | "api-tokens";

/** Tab ids in display order. */
export const PROFILE_SETTINGS_TAB_IDS: readonly ProfileSettingsTabId[] = [
  "general",
  "preferences",
  "ai-assistant",
  "auto-project-management",
  "notifications",
  "security",
  "activity",
  "integrations",
  "api-tokens",
];

const TAB_IDS = new Set<string>(PROFILE_SETTINGS_TAB_IDS);

/** True for one of the nine registered tab ids. */
export function isProfileSettingsTab(value: string): value is ProfileSettingsTabId {
  return TAB_IDS.has(value);
}

/**
 * Resolve a raw address segment to a tab id. A trailing slash is
 * tolerated (in-app links carry one); anything unregistered yields null.
 */
export function resolveProfileSettingsTab(raw: string | undefined): ProfileSettingsTabId | null {
  if (raw === undefined) return null;
  const trimmed = raw.endsWith("/") ? raw.slice(0, -1) : raw;
  return isProfileSettingsTab(trimmed) ? trimmed : null;
}

/** The normalized query string without its leading `?`. */
export function queryWithoutMark(search: string): string {
  return search.startsWith("?") ? search.slice(1) : search;
}

/**
 * Tab address. In-app links carry a trailing slash, matching the old
 * app's link landings; a query string is preserved verbatim.
 */
export function profileSettingsHref(tab: ProfileSettingsTabId, search?: string): string {
  const query = queryWithoutMark(search ?? "");
  return `/settings/profile/${tab}/${query === "" ? "" : `?${query}`}`;
}
