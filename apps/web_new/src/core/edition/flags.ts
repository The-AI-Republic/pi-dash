// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shell edition gates (SHELL-063, SHELL-066, from parity/specs/shell.md
// §5). The rail stays suppressed unless an edition opts in; the sidebar
// toggle stays mounted unless an edition opts out (the community
// default on every build today).
import { APP_RAIL_FLAG, PAGE_MOVE_FLAG, SIDEBAR_TOGGLE_FLAG, type Edition } from "./types.js";

/** True only when the edition opts into the far-left app rail. */
export function isAppRailEnabled(edition: Edition): boolean {
  return edition.flags[APP_RAIL_FLAG] === true;
}

/** False only when the edition explicitly hides the sidebar toggle. */
export function isSidebarToggleEnabled(edition: Edition): boolean {
  return edition.flags[SIDEBAR_TOGGLE_FLAG] !== false;
}

/**
 * True only where the edition's server serves the cross-project page move
 * (PAGE-017). The community default is absent, which is what the oracle
 * pins: no row or detail affordance and no route.
 */
export function isPageMoveEnabled(edition: Edition): boolean {
  return edition.flags[PAGE_MOVE_FLAG] === true;
}
