// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project views tab home (parity/specs/views.md §3, VIEW-001). The views
// area owns this screen (NEWFRONT-719): the saved-views list with its
// gate, states and row identity. The detail route keeps its own host.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

// Deep import: this route module is eager, so the views barrel would pin
// every dialog and screen it re-exports onto the initial chain
// (NEWFRONT-871, size gate).
import { ProjectViewsListScreen } from "../../../../../../features/views/components/ProjectViewsListScreen.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/views/")({
  component: ViewsComponent,
});

function ViewsComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return <ProjectViewsListScreen workspaceSlug={ws} projectId={projectId} />;
}
