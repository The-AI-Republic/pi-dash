// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace all-issues route: the static default's detail address
// (parity/specs/views.md §11, VIEW-038). Thin like its `$viewId` sibling:
// the same detail chunk renders with the static key, so the default keeps
// its issues-owned body (the unfiltered global list of ISS-223 with the
// workspace empty card of ISS-073) inside the views header, tab strip and
// controls. The router prefers this static segment over the param route.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { parseIssueSearch } from "../../../../features/issues/filters/search.js";
import { peekSearchFor, stripPeekParams, type PeekRequest } from "../../../../features/issues/peek/peekUrl.js";
import { ListSkeleton } from "../../../../features/issues/loaders/ListSkeleton.js";
import { projectsQueryOptions } from "../../../../features/projects/api/queries.js";
// Deep import, not the views barrel: this route file is eager (the router
// imports every route), and the barrel re-exports modules with module-level
// state that Rollup then keeps in the initial chunk (NEWFRONT-879; the same
// shape NEWFRONT-869 fixed for the project-views route).
import { loadWorkspaceViewDetail, resolvedWorkspaceViewDetail } from "../../../../features/views/detail/lazy.js";
import type { WorkspacePeekAddress } from "../../../../features/views/detail/WorkspaceViewScreen.js";

// Start the chunk with the route module so the visit does not serialize a
// round trip behind the data reads; `beforeLoad` awaits the same request.
void loadWorkspaceViewDetail().catch(() => undefined);

function AllIssuesRouteComponent(): React.ReactElement {
  const { ws } = Route.useParams();
  const search = Route.useSearch();
  const navigate = useNavigate();
  // The default's body is an issues surface: it binds the same peek
  // address as a project list (ISS-167). Writes replace so Back leaves
  // the view, and closing strips every peek param. The workspace route
  // has no path project, so the peeked project is always written.
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
  return <WorkspaceViewScreen params={{ ws, viewId: "all-issues" }} peek={peek} />;
}

export const Route = createFileRoute("/$ws/_gated/workspace-views/all-issues")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  beforeLoad: async ({ context, params }) => {
    const { preloadWorkspaceView } = await loadWorkspaceViewDetail();
    await preloadWorkspaceView(context.queryClient, context.apiClient, { ws: params.ws, viewId: "all-issues" });
  },
  loader: async ({ context, params }) => {
    await context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws));
  },
  pendingComponent: function AllIssuesPending() {
    return <ListSkeleton />;
  },
  component: AllIssuesRouteComponent,
});
