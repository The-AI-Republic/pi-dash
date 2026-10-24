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
// Deferred below the initial chain (NEWFRONT-871, size gate): the entries
// only register palette commands and their jump page, so they can land
// after the layout paints — the same shape the workspace entries take in
// the gated layout. A static import here would put the views dialog and
// its create-form chain on every project page's first load.
const ViewsPaletteEntries = React.lazy(() =>
  import("../../../../../features/views/palette/ViewsPaletteEntries.js").then((module) => ({
    default: module.ViewsPaletteEntries,
  }))
);

export const Route = createFileRoute("/$ws/_gated/projects/$projectId")({
  component: ProjectLayoutComponent,
});

function ProjectLayoutComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectAccessGate slug={ws} projectId={projectId}>
      {/* The entries carry their own boundary: they register nothing visible,
          so the tab strip and the page must not wait for (or blank behind)
          the chunk's first load (NEWFRONT-871, size gate). */}
      <React.Suspense fallback={null}>
        <ViewsPaletteEntries workspaceSlug={ws} projectId={projectId} />
      </React.Suspense>
      <ProjectTabStrip workspaceSlug={ws} projectId={projectId} />
      <Outlet />
    </ProjectAccessGate>
  );
}
