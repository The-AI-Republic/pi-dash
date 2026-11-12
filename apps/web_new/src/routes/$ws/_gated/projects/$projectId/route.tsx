// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project layout (AUTH-024, SHELL-072-079). Every project page renders
// through the project access gate: members see the page, everyone else
// gets the join, no-access, or not-found screen keyed off the
// details-fetch status plus admin standing. Members also get the
// project tab strip above the page in tabbed mode, and the views and
// modules palette entries (VIEW-042, MOD-037) on every project page.

import { createFileRoute, Outlet, useRouterState } from "@tanstack/react-router";
import * as React from "react";

import { ProjectAccessGate } from "../../../../../features/auth/components/ProjectAccessGate.js";
import { ProjectTabStrip } from "../../../../../features/projects/components/ProjectTabStrip.js";
import { isProjectRunnersRouteId } from "../../../../../features/runners/routeId.js";
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
// Same shape for the modules entries: they register two commands and a
// jump page, and the create dialog they can open stays in its own chunk.
const ModulePaletteEntries = React.lazy(() =>
  import("../../../../../features/modules/palette/ModulePaletteEntries.js").then((module) => ({
    default: module.ModulePaletteEntries,
  }))
);

export const Route = createFileRoute("/$ws/_gated/projects/$projectId")({
  component: ProjectLayoutComponent,
});

function ProjectLayoutComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  // The AI Workers area fills the window below the tab strip (NEWPIDASH-3):
  // on its routes the strip and the page share a full-height column, so
  // the area takes what the strip leaves. Every other tab keeps the plain
  // stacking it always had.
  const fullHeightPage = useRouterState({
    select: (state) => state.matches.some((match) => isProjectRunnersRouteId(match.routeId)),
  });
  const page = (
    <>
      <ProjectTabStrip workspaceSlug={ws} projectId={projectId} />
      <Outlet />
    </>
  );
  return (
    <ProjectAccessGate slug={ws} projectId={projectId}>
      {/* The entries carry their own boundary: they register nothing visible,
          so the tab strip and the page must not wait for (or blank behind)
          the chunk's first load (NEWFRONT-871, size gate). */}
      <React.Suspense fallback={null}>
        <ViewsPaletteEntries workspaceSlug={ws} projectId={projectId} />
      </React.Suspense>
      <React.Suspense fallback={null}>
        <ModulePaletteEntries workspaceSlug={ws} projectId={projectId} />
      </React.Suspense>
      {fullHeightPage ? (
        <div data-testid="project-full-height-page" className="flex h-full min-h-0 flex-col">
          {page}
        </div>
      ) : (
        page
      )}
    </ProjectAccessGate>
  );
}
