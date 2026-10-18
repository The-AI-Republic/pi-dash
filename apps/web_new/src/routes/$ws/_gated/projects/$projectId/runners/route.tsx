// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-scoped runners layout (NEWFRONT-540, RUN-001/002/004). The project
// twin of the workspace runner layout: the area shell decides access and
// renders the breadcrumb chrome; the tab strip arrives from the pathless
// project layout above.

import { createFileRoute, Outlet } from "@tanstack/react-router";
import * as React from "react";

import { selectWorkspace, useWorkspaces } from "../../../../../../core/session/index.js";
import { RunnersProjectShell } from "../../../../../../features/runners/index.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/runners")({
  component: RunnersProjectLayout,
});

function RunnersProjectLayout(): React.ReactElement | null {
  const { ws, projectId } = Route.useParams();
  const workspace = selectWorkspace(useWorkspaces().data, ws);
  if (workspace === undefined) return null;
  return (
    <RunnersProjectShell workspaceSlug={ws} workspaceId={workspace.id} projectId={projectId}>
      <Outlet />
    </RunnersProjectShell>
  );
}
