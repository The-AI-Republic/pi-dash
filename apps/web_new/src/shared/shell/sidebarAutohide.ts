// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar autohide rules (SHELL-062, from parity/specs/shell.md §4).
// The notifications route renders full-width with no app sidebar; the
// gated layout unmounts the sidebar there (and the top bar hides its
// toggle and shows the compact account fallback) while the inbox link
// reads current. Narrow viewports collapse through the shell instead:
// any tap inside the sidebar content collapses it (SidebarShell) and
// every route navigation callback collapses it (gated route).

/** The path segment marking the full-width notifications route. */
export const NOTIFICATIONS_PATH_SEGMENT = "/notifications/";

/** True on the notifications route, where the sidebar stays unmounted. */
export function isNotificationsPath(pathname: string): boolean {
  return pathname.includes(NOTIFICATIONS_PATH_SEGMENT);
}
