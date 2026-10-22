// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One settings tab (parity/specs/profile.md §5, PROF-009; cloud
// withdrawal CLOUD-035). The address names the tab; an unknown id falls
// back to the default tab instead of stranding the page (the
// NEWFRONT-430 target). A tab the instance withholds forwards the same
// way before its body mounts, so a bookmarked password address lands on
// the general tab on cloud-managed instances. The body comes from the
// settings page registry, so this route never names a tab's content.

import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import { SettingsTabOutlet } from "../../../features/settings/components/SettingsTabOutlet.js";
import {
  DEFAULT_PROFILE_SETTINGS_TAB,
  resolveProfileSettingsTab,
} from "../../../features/settings/lib/tabNavigation.js";
import { defaultTabHref } from "./index.js";

/**
 * The address an unknown tab segment forwards to: the default tab with
 * the query string kept, so a stale bookmark still lands on a real page.
 */
export function unknownTabHref(search: string): string {
  return defaultTabHref(search);
}

export const Route = createFileRoute("/settings/profile/$tab")({
  // NOTE (NEWFRONT-852, size gate): the instance-gate read stays behind a
  // dynamic import so it never joins the initial bundle — this gate only
  // runs on settings-tab navigation, after first paint.
  beforeLoad: async ({ context, params, location }) => {
    const tab = resolveProfileSettingsTab(params.tab);
    if (tab === null) throw redirect({ href: unknownTabHref(location.searchStr) });
    const { isTabAddressServed, tabHasInstanceGate } =
      await import("../../../features/settings/lib/tabAvailability.js");
    if (!tabHasInstanceGate(tab)) return;
    const served = await isTabAddressServed({ queryClient: context.queryClient, apiClient: context.apiClient, tab });
    if (!served) throw redirect({ href: unknownTabHref(location.searchStr) });
  },
  component: ProfileSettingsTab,
});

function ProfileSettingsTab(): React.ReactElement {
  const { tab } = Route.useParams();
  const context = Route.useRouteContext();
  const resolved = resolveProfileSettingsTab(tab) ?? DEFAULT_PROFILE_SETTINGS_TAB;
  return <SettingsTabOutlet tab={resolved} selfManaged={context.selfManaged} />;
}
