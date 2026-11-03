// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar autohide rules (SHELL-062, from parity/specs/shell.md §4). The
// notifications route renders full-width with no app sidebar; the
// gated layout unmounts the sidebar there (and the top bar hides its
// toggle and shows the compact account fallback) while the inbox link
// reads current. Narrow viewports collapse through the shell instead:
// any tap inside the sidebar content collapses it (SidebarShell) and
// every route navigation callback collapses it (gated route).

/** The second path segment of the workspace's full-width notifications page. */
export const NOTIFICATIONS_PATH_SEGMENT = "notifications";

/** The second path segment of the per-workspace account-settings area. */
export const ACCOUNT_SETTINGS_SEGMENT = "settings";

function secondSegment(pathname: string): string | null {
  return pathname.split("/").filter(Boolean)[1] ?? null;
}

/**
 * True on the notifications route, where the sidebar stays unmounted.
 * The segment must be the workspace's second one: an account tab such as
 * `/<ws>/settings/account/notifications/` names notifications further
 * along and keeps the ordinary chrome.
 */
export function isNotificationsPath(pathname: string): boolean {
  return secondSegment(pathname) === NOTIFICATIONS_PATH_SEGMENT;
}

/**
 * True inside a workspace's account-settings area (`/<ws>/settings/account…`).
 * Those pages swap the ordinary workspace sidebar for the settings
 * navigator (main#670), so the shell leaves the sidebar unmounted and the
 * top bar shows the compact account fallback; the pane itself stays.
 */
export function isAccountSettingsPathname(pathname: string): boolean {
  const segments = pathname.split("/").filter(Boolean);
  return segments[1] === ACCOUNT_SETTINGS_SEGMENT && segments[2] === "account";
}
