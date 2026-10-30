// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy personal-settings addresses (parity/specs/profile.md §5,
// PROF-026; main#670). Account settings now live inside a workspace, so a
// legacy address only names a tab; its route resolves the workspace and
// redirects to `/<ws>/settings/account/<tab>`. The families are:
//
//   /settings, /settings/*             the signed-in user's workspace
//   /profile, /profile/<tab>           the signed-in user's workspace
//   /<ws>/settings/account[/<tab>]     that workspace
//   /<ws>/settings/api-tokens          that workspace, tokens tab
//
// Only a registered tab id is forwarded as a tab; an absent, unknown or
// mistyped segment lands on the general tab with the query string kept,
// so a stale link never strands the page on a loader.

import { DEFAULT_PROFILE_SETTINGS_TAB, isProfileSettingsTab, type ProfileSettingsTabId } from "./tabNavigation.js";

/** The tab a legacy segment names, or the default for anything else. */
export function legacyTabOf(raw: string | undefined): ProfileSettingsTabId {
  return raw !== undefined && isProfileSettingsTab(raw) ? raw : DEFAULT_PROFILE_SETTINGS_TAB;
}

/**
 * The tab a retained address names, or null when it belongs to no legacy
 * family. Only the workspace-less families reach this resolver: the
 * `/<ws>/settings/...` addresses are real routes of the workspace tree
 * that forward through `accountSettingsHref` themselves.
 */
export function legacySettingsTarget(pathname: string): ProfileSettingsTabId | null {
  const segments = pathname.split("/").filter(Boolean);
  if (segments.length === 0) return null;
  const [head, second, third] = segments;

  if (head === "settings") {
    // `/settings/profile/<tab>` and the bare root; anything else beneath
    // `/settings` is an old leftover and lands on the general tab.
    if (segments.length === 3 && second === "profile") return legacyTabOf(third);
    return DEFAULT_PROFILE_SETTINGS_TAB;
  }

  if (head === "profile") {
    if (segments.length === 1) return DEFAULT_PROFILE_SETTINGS_TAB;
    if (segments.length === 2) return legacyTabOf(second);
  }
  return null;
}
