// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Legacy profile-settings addresses (parity/specs/profile.md §5,
// PROF-026). Four families forward to the new tab addresses, keeping the
// query string and defaulting an absent or unknown tab to the general
// tab. The workspace api-tokens forward keeps its parameters like the
// others (the target for NEWFRONT-431).

import {
  DEFAULT_PROFILE_SETTINGS_TAB,
  isProfileSettingsTab,
  queryWithoutMark,
  type ProfileSettingsTabId,
} from "./tabs.js";

/** The forward target, split so routes can hand it to the router. */
export interface LegacyForward {
  tab: ProfileSettingsTabId;
  /** The query string without its leading `?` (empty when absent). */
  query: string;
  /** Address the browser ends on, trailing slash included. */
  href: string;
}

function forward(rawTab: string | undefined, search: string): LegacyForward {
  const tab = rawTab !== undefined && isProfileSettingsTab(rawTab) ? rawTab : DEFAULT_PROFILE_SETTINGS_TAB;
  const query = queryWithoutMark(search);
  return {
    tab,
    query,
    href: `/settings/profile/${tab}/${query === "" ? "" : `?${query}`}`,
  };
}

/**
 * Map a pathname plus query to the settings address it forwards to, or
 * null when the address belongs to no legacy family. The workspace slug is
 * any single leading segment.
 */
export function legacySettingsForward(pathname: string, search: string): LegacyForward | null {
  const segments = pathname.split("/").filter(Boolean);
  if (segments.length === 0) return null;

  if (segments[0] === "settings") {
    // /settings and /settings/profile/<tab>; longer addresses are not
    // settings forwards (the workspace settings area owns them).
    if (segments.length === 1) return forward(undefined, search);
    if (segments.length === 3 && segments[1] === "profile") return forward(segments[2], search);
    return null;
  }

  if (segments[0] === "profile") {
    // /profile and /profile/<tab>.
    if (segments.length === 1) return forward(undefined, search);
    if (segments.length === 2) return forward(segments[1], search);
    return null;
  }

  // /<workspace>/settings/account[/<tab>] and /<workspace>/settings/api-tokens.
  if (segments.length >= 3 && segments[1] === "settings") {
    if (segments[2] === "account") {
      if (segments.length === 3) return forward(undefined, search);
      if (segments.length === 4) return forward(segments[3], search);
      return null;
    }
    if (segments[2] === "api-tokens" && segments.length === 3) {
      return forward("api-tokens", search);
    }
  }
  return null;
}
