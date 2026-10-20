// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Which profile tabs an instance serves (parity/specs/cloud.md §8,
// CLOUD-035). The password tab belongs to instances that own the
// credential; a cloud-managed instance hands passwords to its account
// home, so the tab disappears from the sidebar and a bookmarked address
// forwards to the default tab. Only a positive cloud answer hides a tab:
// while the instance config is unread the sidebar keeps its full list,
// so a slow read never strands a self-managed operator's password page.
import type { ProfileSettingsTabId } from "./tabNavigation.js";

/** Tabs an instance config can withdraw. */
const CONFIG_GATED_TABS: ReadonlySet<ProfileSettingsTabId> = new Set(["security"]);

/**
 * True when the tab is served here. `selfManaged` is the instance
 * config's marker: `undefined` while unknown, and unknown keeps the
 * tab. A self-managed instance serves the password tab; a hosted
 * (cloud-managed) one withdraws it.
 */
export function isProfileSettingsTabServed(tab: ProfileSettingsTabId, selfManaged: boolean | undefined): boolean {
  if (!CONFIG_GATED_TABS.has(tab)) return true;
  return selfManaged !== false;
}

/** The tab a withdrawn address forwards to. */
export const WITHDRAWN_TAB_FALLBACK: ProfileSettingsTabId = "general";
