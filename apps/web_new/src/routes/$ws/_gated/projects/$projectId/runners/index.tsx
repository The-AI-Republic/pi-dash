// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners overview, project scope (NEWFRONT-540, RUN-002/RUN-005). The same
// overview screen as the workspace twin; the area context carries the project
// scope, so every read and link is project-scoped.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { selectWorkspace, useWorkspaces } from "../../../../../../core/session/index.js";
import { RunnersOverviewScreen } from "../../../../../../features/runners/components/RunnersOverviewScreen.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/runners/")({
  component: RunnersProjectOverviewRoute,
});

function RunnersProjectOverviewRoute(): React.ReactElement | null {
  const { ws } = Route.useParams();
  const workspace = selectWorkspace(useWorkspaces().data, ws);
  return <RunnersOverviewScreen workspaceSlug={ws} workspaceName={workspace?.name} />;
}
