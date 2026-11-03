// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-settings layout (parity/specs/workspace-settings.md §1, §2,
// §36; WSET-001/002/007/036). Pathless so the shell frames only the six
// workspace sections: project settings (`settings/projects/...`) keep
// their own layout. The signed-in workspace gate runs first; this layout
// then resolves the section from the address, refuses out-of-role
// visitors on the same URL with the not-authorized surface, and composes
// the grouped sidebar around the section outlet. Navigation stays
// client-side; no settings page triggers a full reload.

import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Outlet, useRouterState } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../../../core/api/client.js";
import { usePermissions, useWorkspace } from "../../../../../core/session/index.js";
import { resolveSettingsAddress } from "../../../../../features/project-settings/tabs.js";
import { NotFoundPage, resolveFileUrl } from "../../../../../shared/shell/index.js";
import { decideWorkspaceSettingsGate } from "../../../../../features/workspace-settings/lib/gate.js";
import { resolveWorkspaceSettingsIdentity } from "../../../../../features/workspace-settings/lib/identity.js";
import { isWorkspaceWebhookDetailPath } from "../../../../../features/workspace-settings/lib/title.js";
import { SettingsNotAuthorized } from "../../../../../features/workspace-settings/components/SettingsNotAuthorized.js";
import { SettingsShell } from "../../../../../features/workspace-settings/components/SettingsShell.js";
import { workspaceQueryOptions } from "./workspaceQuery.js";

/** Whether the address belongs to the project-settings area. */
function isProjectSettingsPath(pathname: string): boolean {
  const [head, settings, first] = pathname.split("/").filter(Boolean);
  return head !== undefined && settings === "settings" && first === "projects";
}

/**
 * Whether a project-settings address names one of the area's tab routes
 * (EST-026). Resolved against the tab registry, not the segment count:
 * addresses below a tab's own address — a guessed system id, a mistyped
 * suffix — are not part of any tab and get the not-found page.
 */
function hasProjectSettingsTab(pathname: string, workspaceSlug: string): boolean {
  return resolveSettingsAddress(pathname, workspaceSlug) !== undefined;
}

export const Route = createFileRoute("/$ws/_gated/settings/_workspace")({
  component: WorkspaceSettingsLayout,
});

function WorkspaceSettingsLayout(): React.ReactElement {
  const { ws } = Route.useParams();
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const permissions = usePermissions(ws);
  const workspace = useWorkspace(ws);
  const navigate = Route.useNavigate();

  const gate = decideWorkspaceSettingsGate(pathname, permissions);
  const allowed = gate?.status === "allowed";
  // The workspace details decorate the sidebar. Out-of-role visitors
  // never ask for them: the shell owns no other settings traffic (§1).
  const details = useQuery({ ...workspaceQueryOptions(getAppClient(), ws), enabled: allowed });

  const onNavigate = React.useCallback(
    (href: string) => {
      void navigate({ href });
    },
    [navigate]
  );

  // Renders nothing until the role resolves: the gate must not flash
  // content for a viewer who turns out to lack access. The frame covers
  // the whole window here; the `_gated` layout yields its ordinary
  // workspace chrome to the shell (WSET-001/002).
  if (permissions.isLoading) return <React.Fragment />;
  // `settings/projects/...` is the project-settings area (its own shell,
  // its own slice): its tab routes keep the frame and mount below, and
  // its generic addresses get the ordinary not-found page rather than a
  // workspace-settings denial.
  if (isProjectSettingsPath(pathname)) {
    return hasProjectSettingsTab(pathname, ws) ? <Outlet /> : <NotFoundPage />;
  }
  if (gate === null || gate.status === "denied") return <SettingsNotAuthorized workspaceSlug={ws} />;

  const identity = resolveWorkspaceSettingsIdentity({
    workspaceSlug: ws,
    details: details.data,
    listed: workspace.workspace,
  });
  const logoUrl = resolveFileUrl(identity.logoLeg, getAppClient().baseUrl);

  // Full-window frame with no stacking layer of its own: the kit renders
  // dialogs, menus and popovers in portals after this node, so a z-index
  // here would paint the frame over them and swallow their clicks.
  return (
    <div className="fixed inset-0 bg-(--bg) text-(--text)">
      <SettingsShell
        workspaceSlug={ws}
        workspaceName={identity.workspaceName}
        logoUrl={logoUrl}
        role={permissions.role}
        permissions={permissions}
        section={gate.section}
        webhookDetail={isWorkspaceWebhookDetailPath(pathname)}
        onNavigate={onNavigate}
        body={<Outlet />}
      />
    </div>
  );
}
