// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One account-settings tab (parity/specs/profile.md §5, PROF-009; cloud
// withdrawal CLOUD-035). The address names the tab; an unknown id and a
// tab this instance withholds both forward to the general tab with the
// query string kept, so a stale bookmark lands on a real page instead of
// stranding the loader (NEWFRONT-430). The body comes from the settings
// page registry, so this route never names a tab's content.

import { createFileRoute, redirect } from "@tanstack/react-router";
import * as React from "react";

import { SettingsTabOutlet } from "../../../../../features/settings/components/SettingsTabOutlet.js";
import { accountSettingsHref } from "../../../../../features/settings/lib/accountHref.js";
import {
  DEFAULT_PROFILE_SETTINGS_TAB,
  resolveProfileSettingsTab,
} from "../../../../../features/settings/lib/tabNavigation.js";

export const Route = createFileRoute("/$ws/_gated/settings/account/$tab")({
  beforeLoad: async ({ context, params, location }) => {
    const tab = resolveProfileSettingsTab(params.tab);
    if (tab === null) {
      throw redirect({
        href: accountSettingsHref(params.ws, DEFAULT_PROFILE_SETTINGS_TAB, location.searchStr, location.hash),
      });
    }
    // The instance-gate read stays behind a dynamic import so it never
    // joins the initial bundle (NEWFRONT-852, size gate).
    const { isTabAddressServed } = await import("../../../../../features/settings/lib/tabAvailability.js");
    const served = await isTabAddressServed({ queryClient: context.queryClient, apiClient: context.apiClient, tab });
    if (!served) {
      throw redirect({
        href: accountSettingsHref(params.ws, DEFAULT_PROFILE_SETTINGS_TAB, location.searchStr, location.hash),
      });
    }
  },
  component: AccountSettingsTab,
});

function AccountSettingsTab(): React.ReactElement {
  const { tab } = Route.useParams();
  const resolved = resolveProfileSettingsTab(tab) ?? DEFAULT_PROFILE_SETTINGS_TAB;
  return <SettingsTabOutlet tab={resolved} />;
}
