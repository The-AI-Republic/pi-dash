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

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { parseIssueSearch } from "../../../../../features/issues/filters/search.js";
import { peekSearchFor, stripPeekParams, type PeekRequest } from "../../../../../features/issues/peek/peekUrl.js";
import { ListSkeleton } from "../../../../../features/issues/loaders/ListSkeleton.js";
import { projectsQueryOptions } from "../../../../../features/projects/api/queries.js";
// Deep import, not the views barrel: this route file is eager (the router
// imports every route), and the barrel re-exports modules with module-level
// state that Rollup then keeps in the initial chunk (NEWFRONT-879; the same
// shape NEWFRONT-869 fixed for the project-views route).
import { loadWorkspaceViewDetail, resolvedWorkspaceViewDetail } from "../../../../../features/views/detail/lazy.js";
import type { WorkspaceViewDetailParams } from "../../../../../features/views/detail/workspacePreload.js";
import type { WorkspacePeekAddress } from "../../../../../features/views/detail/WorkspaceViewScreen.js";

// Start the chunk with the route module so the visit does not serialize a
// round trip behind the data reads; `beforeLoad` awaits the same request.
void loadWorkspaceViewDetail().catch(() => undefined);

function WorkspaceViewRouteComponent(): React.ReactElement {
  const params = Route.useParams() as WorkspaceViewDetailParams;
  const search = Route.useSearch();
  const navigate = useNavigate();
  // A custom view renders the same issues body, so it binds the same peek
  // address (ISS-167): replace writes, every param stripped on close, and
  // the peeked project written because the route has no path project.
  const setPeek = (request: PeekRequest): void => {
    const next = request === null ? stripPeekParams({ ...search }) : peekSearchFor(search, request, "");
    void navigate({ to: ".", search: next, replace: true } as never);
  };
  const peek: WorkspacePeekAddress = {
    peekKey: search.peekIssueId ?? null,
    peekProjectId: search.peekProjectId ?? null,
    onPeekChange: setPeek,
  };
  const { WorkspaceViewScreen } = resolvedWorkspaceViewDetail();
  return <WorkspaceViewScreen params={params} peek={peek} />;
}

export const Route = createFileRoute("/$ws/_gated/workspace-views/$viewId/")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
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
