// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Account-settings layout inside the workspace shell (parity/specs/
// profile.md §5, PROF-009/010; main#670). `/<ws>/settings/account/<tab>`
// is an ordinary child of the gated workspace tree, so the app rail, top
// bar and palette stay mounted; this layout only places the navigator as
// the middle panel and the tab body beside it. The signed-in gate already
// ran above, so no guard lives here.

import { createFileRoute, Outlet, useRouterState } from "@tanstack/react-router";
import * as React from "react";

import { useInstanceConfig, useWorkspaces } from "../../../../../core/session/index.js";
// Side-effect import: the tab registry fills with the built-in bodies as
// this layout loads, so a direct visit to any tab paints its page.
import "../../../../../features/settings/pageEntries.js";
import { AccountSettingsNavigator } from "../../../../../features/settings/components/account/AccountSettingsNavigator.js";
import { accountSettingsHref } from "../../../../../features/settings/lib/accountHref.js";
import {
  resolveProfileSettingsTab,
  type ProfileSettingsTabId,
} from "../../../../../features/settings/lib/tabNavigation.js";
import { findProfileSettingsTab, profileSettingsTitle } from "../../../../../features/settings/lib/tabs.js";
import { isProfileSettingsTabServed } from "../../../../../features/settings/lib/visibility.js";
import { useDocumentHead } from "../../../../../shared/shell/index.js";

export const Route = createFileRoute("/$ws/_gated/settings/account")({
  component: AccountSettingsLayout,
});

/**
 * The tab a `/<ws>/settings/account/...` address names: the registered id
 * at the fourth segment, or null for the bare collection and unknown ids.
 */
export function accountTabFromPathname(pathname: string, workspaceSlug: string): ProfileSettingsTabId | null {
  const segments = pathname.split("/").filter(Boolean);
  if (segments[0] !== workspaceSlug || segments[1] !== "settings" || segments[2] !== "account") return null;
  return resolveProfileSettingsTab(segments[3]);
}

function AccountSettingsLayout(): React.ReactElement {
  const { ws } = Route.useParams();
  const navigate = Route.useNavigate();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const workspaces = useWorkspaces();
  const instance = useInstanceConfig();
  const tab = accountTabFromPathname(pathname, ws);
  const selfManaged = instance.data?.is_self_managed;

  useDocumentHead({
    title: tab === null ? "Profile settings" : profileSettingsTitle(tab),
    robots: "noindex, nofollow",
  });

  const onNavigate = React.useCallback(
    (href: string) => {
      void navigate({ href });
    },
    [navigate]
  );

  return (
    <div
      data-testid="profile-settings-shell"
      className="flex h-full min-h-0 flex-col bg-(--bg) text-(--text) md:flex-row"
    >
      <AccountSettingsNavigator
        workspaceSlug={ws}
        activeTab={tab ?? "general"}
        workspaces={workspaces.data ?? []}
        activeLabel={tab === null ? null : findProfileSettingsTab(tab).label}
        // The workspace rows read this address for their highlight; on an
        // account address no workspace entry matches, so nothing in the
        // Workspace section reads current while a tab is open (main#675).
        pathname={pathname}
        onNavigate={onNavigate}
        isTabServed={(candidate) => isProfileSettingsTabServed(candidate, selfManaged)}
        tabHref={(candidate) => accountSettingsHref(ws, candidate)}
      />
      <main data-testid="profile-settings-content" className="min-h-0 min-w-0 flex-1 overflow-y-auto">
        <div className="mx-auto w-full max-w-4xl px-(--space-8) py-(--space-6)">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
