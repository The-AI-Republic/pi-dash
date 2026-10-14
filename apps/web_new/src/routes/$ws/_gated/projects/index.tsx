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

import { ProjectsListView } from "../../../../features/projects/index.js";

export const Route = createFileRoute("/$ws/_gated/projects/")({
  component: ProjectsListRoute,
});

export function ProjectsListRoute(): React.ReactElement {
  const { ws } = Route.useParams();
  return <ProjectsListView workspaceSlug={ws} />;
}
