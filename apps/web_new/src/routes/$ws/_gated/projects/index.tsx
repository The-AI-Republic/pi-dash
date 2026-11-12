// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace projects list (SHELL-024 through SHELL-027, SHELL-043
// through SHELL-045). Thin: the gated layout already gates the session,
// so this resolves the workspace slug and mounts the connected view.
// The collection read stays off the loader so the screen renders its
// skeleton window instead of blocking navigation (SHELL-025).

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import type { ProjectsListView as ProjectsListViewComponent } from "../../../../features/projects/components/ProjectsListView.js";

// Deferred (size gate): route files are eager, so the screen must not join
// the initial chunk just because this route can render it.
const ProjectsListView = React.lazy(() =>
  import("../../../../features/projects/components/ProjectsListView.js").then((module) => ({
    default: module.ProjectsListView,
  }))
) as typeof ProjectsListViewComponent;

export const Route = createFileRoute("/$ws/_gated/projects/")({
  component: ProjectsListRoute,
});

function ProjectsListRoute(): React.ReactElement {
  const { ws } = Route.useParams();
  return (
    <React.Suspense fallback={null}>
      <ProjectsListView workspaceSlug={ws} />
    </React.Suspense>
  );
}
