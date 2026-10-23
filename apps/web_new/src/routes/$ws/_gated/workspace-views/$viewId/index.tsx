// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace view detail route (parity/specs/views.md §11, VIEW-038/040).
// Thin: the router imports every route file, so the views-owned screen and
// the reads behind it ride the detail chunk, started here and awaited by
// `beforeLoad` beside the data. The id is a static default key or a custom
// id; the all-issues default keeps its static address beside this param
// route and the router prefers the static segment. A failed view read is
// not a load error; the screen renders the missing-view state (VIEW-038).

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { ListSkeleton } from "../../../../../features/issues/index.js";
import { projectsQueryOptions } from "../../../../../features/projects/index.js";
import {
  loadWorkspaceViewDetail,
  resolvedWorkspaceViewDetail,
  type WorkspaceViewDetailParams,
} from "../../../../../features/views/index.js";

// Start the chunk with the route module so the visit does not serialize a
// round trip behind the data reads; `beforeLoad` awaits the same request.
void loadWorkspaceViewDetail().catch(() => undefined);

function WorkspaceViewRouteComponent(): React.ReactElement {
  const params = Route.useParams() as WorkspaceViewDetailParams;
  const { WorkspaceViewScreen } = resolvedWorkspaceViewDetail();
  return <WorkspaceViewScreen params={params} />;
}

export const Route = createFileRoute("/$ws/_gated/workspace-views/$viewId/")({
  beforeLoad: async ({ context, params }) => {
    const { preloadWorkspaceView } = await loadWorkspaceViewDetail();
    await preloadWorkspaceView(context.queryClient, context.apiClient, params);
  },
  loader: async ({ context, params }) => {
    await context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws));
  },
  pendingComponent: function WorkspaceViewPending() {
    return <ListSkeleton />;
  },
  component: WorkspaceViewRouteComponent,
});
