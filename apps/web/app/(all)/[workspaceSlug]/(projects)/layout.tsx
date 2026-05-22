/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { Outlet } from "react-router";
import { ProjectsAppPowerKProvider } from "@/components/power-k/projects-app-provider";
import { ExtendedProjectSidebar } from "@/components/workspace/sidebar/app/extended-project-sidebar";
import { WorkspaceShell } from "@/components/workspace/workspace-shell";

function WorkspaceLayout() {
  return (
    <>
      <ProjectsAppPowerKProvider />
      <WorkspaceShell extendedSidebar={<ExtendedProjectSidebar />} includePortal>
        <Outlet />
      </WorkspaceShell>
    </>
  );
}

export default observer(WorkspaceLayout);
