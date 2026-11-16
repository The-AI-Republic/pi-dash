// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Which profile tabs an instance serves (parity/specs/cloud.md §8,
// CLOUD-035). A cloud-managed instance hands passwords to its account
// home, so the password tab disappears from the sidebar and a bookmarked
// address forwards. Only a positive cloud answer hides a tab: while the
// instance config is unread the sidebar keeps its full list.
import type { ProfileSettingsTabId } from "./tabNavigation.js";

/**
 * True when the tab is served here. `selfManaged` is the instance
 * config's marker: `undefined` while unknown, and unknown keeps the tab.
 * The password tab is the only config-gated tab today.
 */
export function isProfileSettingsTabServed(tab: ProfileSettingsTabId, selfManaged: boolean | undefined): boolean {
  return tab !== "security" || selfManaged !== false;
}

/** The tab a withdrawn address forwards to. */
export const WITHDRAWN_TAB_FALLBACK: ProfileSettingsTabId = "general";
