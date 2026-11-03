// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners overview, workspace scope (NEWFRONT-540, RUN-002/RUN-005). The
// layout gate and rail sit above; this route resolves the workspace name for
// the tab title and mounts the shared overview screen.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { selectWorkspace, useWorkspaces } from "../../../../core/session/index.js";
import { RunnersOverviewScreen } from "../../../../features/runners/components/RunnersOverviewScreen.js";

export const Route = createFileRoute("/$ws/_gated/runners/")({
  component: RunnersOverviewRoute,
});

function RunnersOverviewRoute(): React.ReactElement | null {
  const { ws } = Route.useParams();
  const workspace = selectWorkspace(useWorkspaces().data, ws);
  if (workspace === undefined) return null;
  return <RunnersOverviewScreen workspaceSlug={ws} workspaceName={workspace.name} />;
}
