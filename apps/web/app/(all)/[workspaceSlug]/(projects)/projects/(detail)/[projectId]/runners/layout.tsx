/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { Outlet, useParams } from "react-router";
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { AppHeader } from "@/components/core/app-header";
import { ContentWrapper } from "@/components/core/content-wrapper";
import { NotAuthorizedView } from "@/components/auth-screens/not-authorized-view";
import { RunnersSideNav } from "@/components/runners/runners-side-nav";
import { useUserPermissions } from "@/hooks/store/user";
import { useWorkspace } from "@/hooks/store/use-workspace";
import { ProjectRunnersHeader } from "./header";

/**
 * Project-scoped AI Workers panel. Reuses the same middle-panel pages as the
 * workspace-wide aggregate (`/<workspace>/runners`) — the route tree in
 * core.ts points both scopes at the same page modules — but renders them inside
 * the project chrome and filters every list to this project via the projectId
 * in the route.
 */
const ProjectRunnersLayout = observer(function ProjectRunnersLayout() {
  const { workspaceSlug, projectId } = useParams<{ workspaceSlug: string; projectId: string }>();
  const { currentWorkspace } = useWorkspace();
  const { workspaceUserInfo, allowPermissions } = useUserPermissions();
  const workspaceId = currentWorkspace?.id;

  // Project role gate, mirroring the workspace panel's ADMIN/MEMBER rule but at
  // project scope: viewing another project's AI Workers requires membership of
  // that project.
  const canViewRunners = allowPermissions(
    [EUserPermissions.ADMIN, EUserPermissions.MEMBER],
    EUserPermissionsLevel.PROJECT,
    workspaceSlug,
    projectId
  );

  if (workspaceUserInfo && !canViewRunners) {
    return (
      <>
        <AppHeader header={<ProjectRunnersHeader />} />
        <ContentWrapper>
          <NotAuthorizedView section="general" className="h-auto" />
        </ContentWrapper>
      </>
    );
  }

  return (
    <>
      <AppHeader header={<ProjectRunnersHeader />} />
      <ContentWrapper>
        <div className="flex h-full w-full overflow-hidden">
          <RunnersSideNav workspaceId={workspaceId} workspaceSlug={workspaceSlug} projectId={projectId} />
          {/* Pages pad themselves (p-6) so full-bleed surfaces like the runner
              chat can span edge-to-edge. */}
          <main className="min-w-0 flex-1 overflow-auto">
            <Outlet />
          </main>
        </div>
      </ContentWrapper>
    </>
  );
});

export default ProjectRunnersLayout;
