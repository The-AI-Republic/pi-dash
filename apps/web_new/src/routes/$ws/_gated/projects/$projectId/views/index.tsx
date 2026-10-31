// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project views tab home (parity/specs/views.md §3, VIEW-001). The views
// area owns this screen (NEWFRONT-719): the saved-views list with its
// gate, states and row identity. The detail route keeps its own host.

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import type { ProjectViewsListScreen as ProjectViewsListScreenComponent } from "../../../../../../features/views/components/ProjectViewsListScreen.js";

// Deferred (size gate): the eager route definition only carries the
// address; the list screen and its dialog chain ride their own chunk.
const ProjectViewsListScreen = React.lazy(() =>
  import("../../../../../../features/views/components/ProjectViewsListScreen.js").then((module) => ({
    default: module.ProjectViewsListScreen,
  }))
) as typeof ProjectViewsListScreenComponent;

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/views/")({
  component: ViewsComponent,
});

function ViewsComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <React.Suspense fallback={null}>
      <ProjectViewsListScreen workspaceSlug={ws} projectId={projectId} />
    </React.Suspense>
  );
}
