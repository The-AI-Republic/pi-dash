// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project layout (AUTH-024, SHELL-072-079). Every project page renders
// through the project access gate: members see the page, everyone else
// gets the join, no-access, or not-found screen keyed off the
// details-fetch status plus admin standing. Members also get the
// project tab strip above the page in tabbed mode, and the views
// palette entries (VIEW-042) on every project page.

import { createFileRoute, Outlet } from "@tanstack/react-router";
import * as React from "react";

import { ProjectAccessGate } from "../../../../../features/auth/index.js";
import { ProjectTabStrip } from "../../../../../features/projects/index.js";
import { ViewsPaletteEntries } from "../../../../../features/views/index.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId")({
  component: ProjectLayoutComponent,
});

function ProjectLayoutComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectAccessGate slug={ws} projectId={projectId}>
      <ViewsPaletteEntries workspaceSlug={ws} projectId={projectId} />
      <ProjectTabStrip workspaceSlug={ws} projectId={projectId} />
      <Outlet />
    </ProjectAccessGate>
  );
}
