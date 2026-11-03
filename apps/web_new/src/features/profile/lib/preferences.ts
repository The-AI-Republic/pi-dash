// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue-tab layout and display choices (parity/specs/profile.md §2,
// PROF-006). The spec pins them to browser-local storage scoped to the
// workspace plus the viewed member — there is no server endpoint — so the
// profile tabs read and write through the shared preference helpers and
// survive a reload. Unknown stored values fall back to the defaults.

import * as React from "react";

import type { DisplayCalendarPrefs, DisplayPrefs, LayoutKind } from "../../issues/index.js";
import { loadPreference, storePreference } from "../../../shared/shell/preferenceStorage.js";

export type ProfileTabLayout = Extract<LayoutKind, "list" | "kanban">;

const PROFILE_CALENDAR: DisplayCalendarPrefs = { mode: "month", showWeekends: true };

/** The two layouts the profile issue tabs offer. */
export const PROFILE_TAB_LAYOUTS: readonly ProfileTabLayout[] = ["list", "kanban"];

export interface ProfileDisplayState {
  layout: ProfileTabLayout;
  groupBy: string | null;
  orderBy: string;
  showEmptyGroups: boolean;
  showSubIssues: boolean;
  properties: Record<string, boolean>;
}

/** Profile tabs default to list, ungrouped, continuous manual order. */
export const DEFAULT_PROFILE_DISPLAY: ProfileDisplayState = {
  layout: "list",
  groupBy: null,
  orderBy: "sort_order",
  showEmptyGroups: false,
  showSubIssues: false,
  properties: {},
};

/** Storage key: workspace plus viewed member, one record per profile. */
export function profileDisplayKey(workspaceSlug: string, userId: string): string {
  return `pidash:profile-display:${workspaceSlug}:${userId}:v1`;
}

function isLayout(value: unknown): value is ProfileTabLayout {
  return value === "list" || value === "kanban";
}

function isPropertyMap(value: unknown): value is Record<string, boolean> {
  return (
    typeof value === "object" && value !== null && Object.values(value).every((entry) => typeof entry === "boolean")
  );
}

/** Stored record or the defaults; partially written records fill the gaps. */
export function readProfileDisplay(workspaceSlug: string, userId: string): ProfileDisplayState {
  const stored = loadPreference<Partial<ProfileDisplayState>>(
    profileDisplayKey(workspaceSlug, userId),
    {},
    (value): value is Partial<ProfileDisplayState> => typeof value === "object" && value !== null
  );
  return {
    layout: isLayout(stored.layout) ? stored.layout : DEFAULT_PROFILE_DISPLAY.layout,
    groupBy: typeof stored.groupBy === "string" ? stored.groupBy : null,
    orderBy: typeof stored.orderBy === "string" ? stored.orderBy : DEFAULT_PROFILE_DISPLAY.orderBy,
    showEmptyGroups:
      typeof stored.showEmptyGroups === "boolean" ? stored.showEmptyGroups : DEFAULT_PROFILE_DISPLAY.showEmptyGroups,
    showSubIssues:
      typeof stored.showSubIssues === "boolean" ? stored.showSubIssues : DEFAULT_PROFILE_DISPLAY.showSubIssues,
    properties: isPropertyMap(stored.properties) ? stored.properties : {},
  };
}

/** Merge one change into the stored record and return the next state. */
export function writeProfileDisplay(
  workspaceSlug: string,
  userId: string,
  patch: Partial<ProfileDisplayState>
): ProfileDisplayState {
  const next = { ...readProfileDisplay(workspaceSlug, userId), ...patch };
  storePreference(profileDisplayKey(workspaceSlug, userId), next);
  return next;
}

/**
 * Live profile display state: the stored record on mount, every write
 * persisted and reflected immediately. Nested components read the same
 * record the tab renders from.
 */
export function useProfileDisplay(
  workspaceSlug: string,
  userId: string
): [ProfileDisplayState, (patch: Partial<ProfileDisplayState>) => void] {
  const [state, setState] = React.useState<ProfileDisplayState>(() => readProfileDisplay(workspaceSlug, userId));
  React.useEffect(() => {
    setState(readProfileDisplay(workspaceSlug, userId));
  }, [workspaceSlug, userId]);
  const update = React.useCallback(
    (patch: Partial<ProfileDisplayState>) => {
      setState(writeProfileDisplay(workspaceSlug, userId, patch));
    },
    [workspaceSlug, userId]
  );
  return [state, update];
}

/** The profile record as the shared display panel reads it. */
export function toDisplayPrefs(state: ProfileDisplayState): DisplayPrefs {
  return {
    layout: state.layout,
    groupBy: state.groupBy,
    subGroupBy: null,
    orderBy: state.orderBy,
    showEmptyGroups: state.showEmptyGroups,
    showSubIssues: state.showSubIssues,
    calendar: PROFILE_CALENDAR,
    properties: state.properties,
  };
}
