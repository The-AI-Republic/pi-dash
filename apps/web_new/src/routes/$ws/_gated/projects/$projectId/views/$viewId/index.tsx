// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project view detail route (parity/specs/views.md §7, VIEW-021–024, 028).
// Thin: the router imports every route file, so the views-owned screen and
// the reads behind it ride the detail chunk, started here and awaited by
// `beforeLoad` beside the data. A failed view read is not a load error; the
// screen renders the missing-view state (VIEW-022).

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { isProjectGateError } from "../../../../../../../features/auth/denials.js";
// Deep imports, so the eager route module does not take the issues barrel
// with it (NEWFRONT-871, size gate).
import { parseIssueSearch } from "../../../../../../../features/issues/filters/search.js";
import { ListSkeleton } from "../../../../../../../features/issues/loaders/ListSkeleton.js";
import { projectsQueryOptions } from "../../../../../../../features/projects/api/queries.js";
// Deep import, not the views barrel: this route file is eager (the router
// imports every route), and the barrel re-exports modules with module-level
// state that Rollup then keeps in the initial chunk (NEWFRONT-869, the same
// shape NEWFRONT-867 fixed for the browse route's agent actions).
import { loadSavedViewDetail, resolvedSavedViewDetail } from "../../../../../../../features/views/detail/lazy.js";
import type { SavedViewDetailParams } from "../../../../../../../features/views/detail/preload.js";

// Start the chunk with the route module so the visit does not serialize a
// round trip behind the data reads; `beforeLoad` awaits the same request.
void loadSavedViewDetail().catch(() => undefined);

function SavedViewRouteComponent(): React.ReactElement {
  const params = Route.useParams() as SavedViewDetailParams;
  const search = Route.useSearch();
  const { SavedViewScreen } = resolvedSavedViewDetail();
  return <SavedViewScreen params={params} search={search} />;
}

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/views/$viewId/")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  beforeLoad: async ({ context, params, search }) => {
    try {
      const { preloadSavedView } = await loadSavedViewDetail();
      await preloadSavedView(context.queryClient, context.apiClient, params, search);
    } catch (error) {
      // Only the layout chunk can reject here; gate denials belong to the
      // project layout's denial screen.
      if (!isProjectGateError(error)) throw error;
    }
  },
  loader: async ({ context, params }) => {
    await context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws));
  },
  pendingComponent: function SavedViewPending() {
    return <ListSkeleton />;
  },
  component: SavedViewRouteComponent,
});
