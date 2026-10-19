// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Personal-settings layout (parity/specs/profile.md §5, PROF-009/010).
// Pathless: the shell and its tab route share one grouped sidebar, one
// toast host and one signed-in gate. Each tab body registers through the
// settings feature's page registry, so sibling slices mount inside this
// frame without touching it. No dialog variant exists (main#573 target).

import { ToastHost } from "@pidash/kit";
import { createFileRoute, Outlet, useRouterState } from "@tanstack/react-router";
import * as React from "react";

import { workspacesQueryOptions } from "../../../core/session/index.js";
import { useProfileSettingsSessionGuard } from "../../../features/settings/pageEntries.js";
import { ProfileSettingsShell } from "../../../features/settings/components/ProfileSettingsShell.js";
import {
  DEFAULT_PROFILE_SETTINGS_TAB,
  resolveProfileSettingsTab,
  type ProfileSettingsTabId,
} from "../../../features/settings/lib/tabNavigation.js";
import { getToastManager } from "../../../shared/shell/toaster.js";

/** The tab id a settings address names; null for the layout's bare index. */
export function tabFromPathname(pathname: string): ProfileSettingsTabId | null {
  const segments = pathname.split("/").filter(Boolean);
  if (segments[0] !== "settings" || segments[1] !== "profile") return null;
  return segments[2] === undefined ? null : resolveProfileSettingsTab(segments[2]);
}

export const Route = createFileRoute("/settings/profile")({
  beforeLoad: async ({ context, location }) => {
    // Loaded with the page, not with the router: the gate pulls the whole
    // signed-in decision tree, which no other first paint needs.
    const { loadProfileSettingsLayout } = await import("../../../features/settings/loadProfileSettings.js");
    const { workspaces } = await loadProfileSettingsLayout({
      queryClient: context.queryClient,
      apiClient: context.apiClient,
      href: `${location.pathname}${location.searchStr}`,
    });
    return { workspaces };
  },
  component: ProfileSettingsLayout,
});

function ProfileSettingsLayout(): React.ReactElement {
  useProfileSettingsSessionGuard();
  const navigate = Route.useNavigate();
  const context = Route.useRouteContext();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const tab = tabFromPathname(pathname) ?? DEFAULT_PROFILE_SETTINGS_TAB;
  const workspaces = context.queryClient.getQueryData(workspacesQueryOptions(context.apiClient).queryKey) ?? [];
  const onNavigate = React.useCallback(
    (href: string) => {
      void navigate({ href });
    },
    [navigate]
  );

  return (
    <>
      <ProfileSettingsShell activeTab={tab} workspaces={workspaces} onNavigate={onNavigate}>
        <Outlet />
      </ProfileSettingsShell>
      <ToastHost manager={getToastManager()} />
    </>
  );
}
