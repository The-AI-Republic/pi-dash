// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar link clicks (parity/specs/shell.md §4, SHELL-046–050). Plain
// left clicks stay client-side through the router; modified clicks
// keep their native new-tab behavior.

/** The click facts the takeover rule reads: a subset of MouseEvent. */
export interface SidebarClick {
  button: number;
  metaKey: boolean;
  ctrlKey: boolean;
}

/**
 * True when the sidebar handles the click itself. Modified left
 * clicks (new-tab gestures) stay native; everything else — plain
 * left clicks included — is taken over for client-side navigation.
 */
export function isClientSideClick(event: SidebarClick): boolean {
  return !((event.metaKey || event.ctrlKey) && event.button === 0);
}

/** Active rule for the home row: the bare workspace address (SHELL-046). */
export function isHomeRowActive(pathname: string, workspaceSlug: string): boolean {
  const bare = pathname.endsWith("/") && pathname.length > 1 ? pathname.slice(0, -1) : pathname;
  return bare === `/${workspaceSlug}`;
}
