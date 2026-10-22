// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived work-items tab (parity/specs/issues.md §8, ISS-071). Thin: the
// archives area owns the tab shell and replaces this mount when it lands
// (NEWFRONT-698–700); this route only makes the address render so the
// empty card's row has a real page. The project read starts in `beforeLoad`
// and the screen rides its own chunk, because the route definition is part
// of the eager route graph (Quality gates).

import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

const LazyArchivedIssuesScreen = React.lazy(() =>
  import("../../../../../../features/issues/contexts/ArchivedIssuesScreen.js").then((module) => ({
    default: module.ArchivedIssuesScreen,
  }))
);

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/archives/issues")({
  loader: async ({ context, params }) => {
    const { prefetchArchivedIssues } = await import("../../../../../../features/issues/contexts/prefetchContexts.js");
    await prefetchArchivedIssues({
      queryClient: context.queryClient,
      apiClient: context.apiClient,
      workspaceSlug: params.ws,
      projectId: params.projectId,
    });
  },
  component: ArchivedIssuesRoute,
});

function ArchivedIssuesRoute(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <React.Suspense fallback={null}>
      <LazyArchivedIssuesScreen workspaceSlug={ws} projectId={projectId} />
    </React.Suspense>
  );
}
