// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile-settings sidebar model (parity/specs/profile.md §5, PROF-010).
// Groups the tab ids from tabNavigation.ts under the headings and sidebar
// labels the shell renders. Only the shell needs this table; route guards
// import tabNavigation.ts directly.

import { PROFILE_SETTINGS_TAB_IDS, type ProfileSettingsTabId } from "./tabNavigation.js";

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

/** Sidebar label for each tab id, parallel to PROFILE_SETTINGS_TAB_IDS. */
const TAB_LABELS: Readonly<Record<ProfileSettingsTabId, string>> = {
  general: "Profile",
  preferences: "Preferences",
  "ai-assistant": "AI Assistant",
  "auto-project-management": "Auto Project Management",
  notifications: "Notifications",
  security: "Security",
  activity: "Activity",
  integrations: "Integrations",
  "api-tokens": "Personal Access Tokens",
};

const DEVELOPER_TABS: ReadonlySet<ProfileSettingsTabId> = new Set(["integrations", "api-tokens"]);

/** Tabs in display order with their group. */
export const PROFILE_SETTINGS_TABS: readonly ProfileSettingsTab[] = PROFILE_SETTINGS_TAB_IDS.map((id) => ({
  id,
  label: TAB_LABELS[id],
  group: DEVELOPER_TABS.has(id) ? "developer" : "personal",
}));

/** The tab entry for an id. */
export function findProfileSettingsTab(id: ProfileSettingsTabId): ProfileSettingsTab {
  const tab = PROFILE_SETTINGS_TABS.find((entry) => entry.id === id);
  // The union above is exhaustive over the list, so this is unreachable.
  if (tab === undefined) throw new Error(`Unknown profile settings tab: ${id}`);
  return tab;
}

/**
 * Sidebar groups with their tabs, in display order. Withholding a tab
 * drops it from this list; a group left empty is left out instead of
 * rendering a heading over nothing.
 */
export function profileSettingsGroups(
  isServed: (tab: ProfileSettingsTabId) => boolean = () => true
): Array<ProfileSettingsGroup & { tabs: ProfileSettingsTab[] }> {
  return PROFILE_SETTINGS_GROUPS.map((group) => ({
    ...group,
    tabs: PROFILE_SETTINGS_TABS.filter((tab) => tab.group === group.id && isServed(tab.id)),
  })).filter((group) => group.tabs.length > 0);
}

/** The document title for a tab: the sidebar label plus the page name. */
export function profileSettingsTitle(tab: ProfileSettingsTabId): string {
  return `${findProfileSettingsTab(tab).label} · Profile settings`;
}
