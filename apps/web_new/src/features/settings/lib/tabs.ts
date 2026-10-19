// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile-settings tab registry (parity/specs/profile.md §5, PROF-009/010).
// Every tab owns one address under /settings/profile/<tab>; the shell
// renders the grouped navigation from this list and sibling slices mount
// their body through lib/registry.ts. Unknown ids resolve to null so the
// route can fall back to the default tab instead of stranding the page
// (the target for NEWFRONT-430).

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

export type ProfileSettingsGroupId = "personal" | "developer";

export interface ProfileSettingsTab {
  id: ProfileSettingsTabId;
  /** Sidebar label and document title segment. */
  label: string;
  group: ProfileSettingsGroupId;
}

export interface ProfileSettingsGroup {
  id: ProfileSettingsGroupId;
  label: string;
}

/** Sidebar groups in display order. */
export const PROFILE_SETTINGS_GROUPS: readonly ProfileSettingsGroup[] = [
  { id: "personal", label: "Your Profile" },
  { id: "developer", label: "Developer" },
];

/** Tabs in display order within their group. */
export const PROFILE_SETTINGS_TABS: readonly ProfileSettingsTab[] = [
  { id: "general", label: "Profile", group: "personal" },
  { id: "preferences", label: "Preferences", group: "personal" },
  { id: "ai-assistant", label: "AI Assistant", group: "personal" },
  { id: "auto-project-management", label: "Auto Project Management", group: "personal" },
  { id: "notifications", label: "Notifications", group: "personal" },
  { id: "security", label: "Security", group: "personal" },
  { id: "activity", label: "Activity", group: "personal" },
  { id: "integrations", label: "Integrations", group: "developer" },
  { id: "api-tokens", label: "Personal Access Tokens", group: "developer" },
];

const TAB_IDS = new Set<string>(PROFILE_SETTINGS_TABS.map((tab) => tab.id));

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

/** The tab entry for an id. */
export function findProfileSettingsTab(id: ProfileSettingsTabId): ProfileSettingsTab {
  const tab = PROFILE_SETTINGS_TABS.find((entry) => entry.id === id);
  // The union above is exhaustive over the list, so this is unreachable.
  if (tab === undefined) throw new Error(`Unknown profile settings tab: ${id}`);
  return tab;
}

/** Sidebar groups with their tabs, in display order. */
export function profileSettingsGroups(): Array<ProfileSettingsGroup & { tabs: ProfileSettingsTab[] }> {
  return PROFILE_SETTINGS_GROUPS.map((group) => ({
    ...group,
    tabs: PROFILE_SETTINGS_TABS.filter((tab) => tab.group === group.id),
  }));
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

/** The document title for a tab. */
export function profileSettingsTitle(tab: ProfileSettingsTabId): string {
  return `${findProfileSettingsTab(tab).label} · Profile settings`;
}
