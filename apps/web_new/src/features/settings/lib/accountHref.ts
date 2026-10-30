// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Account-settings address model (parity/specs/profile.md §5, PROF-009/
// 010/026; main#670). Personal settings live inside the workspace they
// were opened from, one tab per address. Route guards, legacy forwards and
// the skeleton need only ids and addresses, so this module stays free of
// labels — the route tree imports it on first paint.

import { DEFAULT_PROFILE_SETTINGS_TAB, type ProfileSettingsTabId, queryWithoutMark } from "./tabNavigation.js";

/** The legacy default tab's address; kept for older callers and tests. */
export { DEFAULT_PROFILE_SETTINGS_TAB } from "./tabNavigation.js";

/** Legacy personal-settings root (outside any workspace). */
export const LEGACY_SETTINGS_ROOT = "/settings";

/**
 * The tab addressed by `/<ws>/settings/account/<tab>/`. A query is kept
 * verbatim; a fragment (old links pointed at `#voice-dictation`) rides
 * along so the landing page opens the same section.
 */
export function accountSettingsHref(
  workspaceSlug: string,
  tab: ProfileSettingsTabId,
  search?: string,
  hash?: string
): string {
  const query = queryWithoutMark(search ?? "");
  const fragment = (hash ?? "").replace(/^#/, "");
  return `/${workspaceSlug}/settings/account/${tab}/${query === "" ? "" : `?${query}`}${
    fragment === "" ? "" : `#${fragment}`
  }`;
}

/** The general-tab address in a workspace. */
export function accountSettingsGeneralHref(workspaceSlug: string): string {
  return accountSettingsHref(workspaceSlug, DEFAULT_PROFILE_SETTINGS_TAB);
}

/**
 * Whether `pathname` is inside one workspace's account-settings area.
 * Used by the navigator to keep the workspace section unlit while account
 * settings are open, and by the shell to name the panel.
 */
export function isAccountSettingsPath(pathname: string, workspaceSlug?: string): boolean {
  const segments = pathname.split("/").filter(Boolean);
  const [head, settings, account] = segments;
  if (head === undefined || settings !== "settings" || account !== "account") return false;
  return workspaceSlug === undefined || head === workspaceSlug;
}

/**
 * The tab a `/<ws>/settings/account/...` address names; null when the
 * address is inside no workspace or names no account tab.
 */
export function accountSettingsWorkspace(pathname: string): string | null {
  const [head, settings, account] = pathname.split("/").filter(Boolean);
  if (head === undefined || settings !== "settings" || account !== "account") return null;
  return head;
}
