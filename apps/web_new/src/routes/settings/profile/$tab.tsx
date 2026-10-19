// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One settings tab (parity/specs/profile.md §5, PROF-009). The address names
// the tab; an unknown id falls back to the default tab instead of
// stranding the page (the NEWFRONT-430 target). The body comes from the
// settings page registry, so this route never names a tab's content.

import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import { SettingsTabOutlet } from "../../../features/settings/components/SettingsTabOutlet.js";
import { DEFAULT_PROFILE_SETTINGS_TAB, resolveProfileSettingsTab } from "../../../features/settings/lib/tabNavigation.js";
import { defaultTabHref } from "./index.js";

/**
 * The address an unknown tab segment forwards to: the default tab with
 * the query string kept, so a stale bookmark still lands on a real page.
 */
export function unknownTabHref(search: string): string {
  return defaultTabHref(search);
}

export const Route = createFileRoute("/settings/profile/$tab")({
  beforeLoad: ({ params, location }) => {
    if (resolveProfileSettingsTab(params.tab) !== null) return;
    throw redirect({ href: unknownTabHref(location.searchStr) });
  },
  component: ProfileSettingsTab,
});

function ProfileSettingsTab(): React.ReactElement {
  const { tab } = Route.useParams();
  const resolved = resolveProfileSettingsTab(tab) ?? DEFAULT_PROFILE_SETTINGS_TAB;
  return <SettingsTabOutlet tab={resolved} />;
}
