/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { Outlet, useParams } from "react-router";
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { ProjectsAppPowerKProvider } from "@/components/power-k/projects-app-provider";
import { NotAuthorizedView } from "@/components/auth-screens/not-authorized-view";
import { RunnersSideNav } from "@/components/runners/runners-side-nav";
import { WorkspaceShell } from "@/components/workspace/workspace-shell";
import { useUserPermissions } from "@/hooks/store/user";
import { useWorkspace } from "@/hooks/store/use-workspace";

const RunnersLayout = observer(function RunnersLayout() {
  const { workspaceSlug } = useParams<{ workspaceSlug: string }>();
  const { currentWorkspace } = useWorkspace();
  const { workspaceUserInfo, allowPermissions } = useUserPermissions();
  const workspaceId = currentWorkspace?.id;

  const canViewRunners = allowPermissions(
    [EUserPermissions.ADMIN, EUserPermissions.MEMBER],
    EUserPermissionsLevel.WORKSPACE
  );

  if (workspaceUserInfo && !canViewRunners) {
    return (
      <>
        {/* Mounts the workspace command palette + shared modal hosts (incl. the
            "New work item" create modal) — runners sits outside the (projects)
            layout, so without this the sidebar "New work item" button is inert. */}
        <ProjectsAppPowerKProvider />
        <WorkspaceShell>
          <NotAuthorizedView section="general" className="h-auto" />
        </WorkspaceShell>
      </>
    );
  }

  return (
    <>
      {/* Mounts the workspace command palette + shared modal hosts (incl. the
          "New work item" create modal) — runners sits outside the (projects)
          layout, so without this the sidebar "New work item" button is inert. */}
      <ProjectsAppPowerKProvider />
      <WorkspaceShell>
        <div className="flex h-full w-full overflow-hidden">
          <RunnersSideNav workspaceId={workspaceId} workspaceSlug={workspaceSlug} />
          {/* Pages pad themselves (p-6) so full-bleed surfaces like the runner
              chat can span edge-to-edge. */}
          <main className="min-w-0 flex-1 overflow-auto">
            <Outlet />
          </main>
        </div>
      </WorkspaceShell>
    </>
  );
});

export default RunnersLayout;
