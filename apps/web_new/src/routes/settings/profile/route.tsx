// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Personal-settings layout (parity/specs/profile.md §5, PROF-009/010).
// Pathless: the shell and its tab route share one grouped sidebar, one
// toast host and one signed-in gate. Each tab body registers through the
// settings feature's page registry, so sibling slices mount inside this
// frame without touching it. No dialog variant exists (main#573 target).

import { ToastHost } from "@pidash/kit";
import type { QueryClient } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";
import { createFileRoute, Outlet, useRouterState } from "@tanstack/react-router";
import * as React from "react";

import { workspacesQueryOptions } from "../../../core/session/index.js";
import { ProfileSettingsShell } from "../../../features/settings/components/ProfileSettingsShell.js";
import { enforceProfileSettingsGate } from "../../../features/settings/guard.js";
import {
  DEFAULT_PROFILE_SETTINGS_TAB,
  findProfileSettingsTab,
  resolveProfileSettingsTab,
  type ProfileSettingsTabId,
} from "../../../features/settings/lib/tabs.js";
import "../../../features/settings/pageEntries.js";
import { getToastManager } from "../../../shared/shell/index.js";

/**
 * The layout's loader: gate the page, then warm the shortcut list. A
 * failed warm leaves the sidebar's workspace section empty; it never
 * blocks the settings page.
 */
export async function loadProfileSettingsLayout(args: {
  queryClient: QueryClient;
  apiClient: ApiClient;
  href: string;
}): Promise<void> {
  await enforceProfileSettingsGate(args);
  await args.queryClient.ensureQueryData(workspacesQueryOptions(args.apiClient)).catch(() => undefined);
}

export const Route = createFileRoute("/settings/profile")({
  beforeLoad: async ({ context, location }) => {
    await loadProfileSettingsLayout({
      queryClient: context.queryClient,
      apiClient: context.apiClient,
      href: `${location.pathname}${location.searchStr}`,
    });
  },
  component: ProfileSettingsLayout,
});

/** The tab id a settings address names; null for the layout's bare index. */
export function tabFromPathname(pathname: string): ProfileSettingsTabId | null {
  const segments = pathname.split("/").filter(Boolean);
  if (segments[0] !== "settings" || segments[1] !== "profile") return null;
  return segments[2] === undefined ? null : resolveProfileSettingsTab(segments[2]);
}

function ProfileSettingsLayout(): React.ReactElement {
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
      <ProfileSettingsShell activeTab={findProfileSettingsTab(tab).id} workspaces={workspaces} onNavigate={onNavigate}>
        <Outlet />
      </ProfileSettingsShell>
      <ToastHost manager={getToastManager()} />
    </>
  );
}
