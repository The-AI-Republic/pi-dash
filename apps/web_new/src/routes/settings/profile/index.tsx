// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bare settings address (parity/specs/profile.md §5, PROF-009): forward to
// the default tab, keeping the query string.

import { createFileRoute, redirect } from "@tanstack/react-router";

import { DEFAULT_PROFILE_SETTINGS_TAB, profileSettingsHref } from "../../../features/settings/lib/tabs.js";

/** The default tab's address, carrying the query string verbatim. */
export function defaultTabHref(search: string): string {
  return profileSettingsHref(DEFAULT_PROFILE_SETTINGS_TAB, search);
}

export const Route = createFileRoute("/settings/profile/")({
  beforeLoad: ({ location }) => {
    throw redirect({ href: defaultTabHref(location.searchStr) });
  },
});
