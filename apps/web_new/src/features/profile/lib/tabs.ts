// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile tab table and role visibility (parity/specs/profile.md §1,
// PROF-002). Everyone sees the summary tab; admins and members also see
// the three issue tabs and the activity tab. Guests reach the summary
// alone and meet an inline denial on the three issue-tab addresses; the
// activity address has no denial (only its download is gated). This
// module is pure so the frame, the picker and route tests share one rule.

import type { Permissions } from "../../../core/session/permissions.js";

export type ProfileTabKey = "summary" | "assigned" | "created" | "subscribed" | "activity";

export interface ProfileTab {
  key: ProfileTabKey;
  label: string;
  /** Address child under `/{ws}/profile/{userId}/`; empty for the summary. */
  child: "" | "assigned" | "created" | "subscribed" | "activity";
  /** Visible to guests too. */
  everyone: boolean;
}

/** Tabs in strip order. */
export const PROFILE_TABS: readonly ProfileTab[] = [
  { key: "summary", label: "Summary", child: "", everyone: true },
  { key: "assigned", label: "Assigned", child: "assigned", everyone: false },
  { key: "created", label: "Created", child: "created", everyone: false },
  { key: "subscribed", label: "Subscribed", child: "subscribed", everyone: false },
  { key: "activity", label: "Activity", child: "activity", everyone: false },
];

/** Issue tabs guests are denied inline when they open the address. */
export const PROFILE_DENIED_TABS: readonly ProfileTabKey[] = ["assigned", "created", "subscribed"];

/** Tabs the workspace role may see. */
export function visibleProfileTabs(permissions: Pick<Permissions, "canEdit">): ProfileTab[] {
  return PROFILE_TABS.filter((tab) => tab.everyone || permissions.canEdit);
}

/** Resolve a route segment to its tab; unknown ids answer null. */
export function profileTabBySegment(segment: string): ProfileTab | null {
  if (segment === "") return null;
  return PROFILE_TABS.find((tab) => tab.child === segment) ?? null;
}

/** Absolute base of the profile page including the trailing slash. */
export function profileBase(workspaceSlug: string, userId: string): string {
  return `/${workspaceSlug}/profile/${userId}/`;
}

/** Address of one tab. */
export function profileTabHref(workspaceSlug: string, userId: string, tab: ProfileTab): string {
  return tab.child === "" ? profileBase(workspaceSlug, userId) : `${profileBase(workspaceSlug, userId)}${tab.child}/`;
}

/**
 * Which tab a pathname selects. Trailing slashes are ignored so the
 * address the app links to and the address a browser lands on agree
 * (the target of NEWFRONT-433). Unknown segments answer null.
 */
export function activeProfileTabKey(pathname: string, workspaceSlug: string, userId: string): ProfileTabKey | null {
  const base = profileBase(workspaceSlug, userId).replace(/\/+$/, "");
  const current = pathname.replace(/\/+$/, "");
  if (current === base) return "summary";
  for (const tab of PROFILE_TABS) {
    if (tab.child !== "" && current === `${base}/${tab.child}`) return tab.key;
  }
  return null;
}
