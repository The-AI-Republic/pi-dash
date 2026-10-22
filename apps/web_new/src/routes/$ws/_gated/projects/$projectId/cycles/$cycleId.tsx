// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle detail host for the issues list (parity/specs/issues.md §2,
// ISS-007–014). The Cycles area owns the full page; this slice proves
// cycle-context grouping, group-header create/attach and the read-only
// completed state here by mounting the shared list layout. The stored
// start/end dates render above the list in the member timezone
// (PROF-017 stored-dates fix, NEWFRONT-859).

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

// Direct module imports: this route definition is eager, and the feature
// barrel would pull the analytics entry and picker parts into the initial
// bundle (NEWFRONT-619, size gate).
import { CycleMemberDates } from "../../../../../../features/cycles/components/CycleMemberDates.js";
import { parseIssueSearch } from "../../../../../../features/issues/filters/search.js";
import { ListSkeleton } from "../../../../../../features/issues/loaders/ListSkeleton.js";

// The list host and its paging module are part of the list layout chunk;
// importing them statically would pull them into the entry chunk through
// the generated route tree (Quality gates).
const LazyEntityListScreen = React.lazy(() =>
  import("../../../../../../features/issues/list/EntityListScreen.js").then((module) => ({
    default: module.EntityListScreen,
  }))
);

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/cycles/$cycleId")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  beforeLoad: async ({ context, params }) => {
    const { prefetchEntityList } = await import("../../../../../../features/issues/list/prefetch.js");
    const entity = { scope: "cycle", projectId: params.projectId, cycleId: params.cycleId } as const;
    await prefetchEntityList(context, params.ws, entity);
  },
  pendingComponent: function CycleIssuesPending() {
    return <ListSkeleton />;
  },
  component: function CycleIssuesScreen() {
    const params = Route.useParams();
    const search = Route.useSearch();
    const navigate = useNavigate();
    const entity = { scope: "cycle", projectId: params.projectId, cycleId: params.cycleId } as const;
    return (
      <>
        <CycleMemberDates workspaceSlug={params.ws} projectId={params.projectId} cycleId={params.cycleId} />
        <React.Suspense fallback={<ListSkeleton />}>
          <LazyEntityListScreen
            workspaceSlug={params.ws}
            entity={entity}
            projectId={params.projectId}
            cycleId={params.cycleId}
            peekKey={search.peekIssueId ?? null}
            onPeekChange={(key) => {
              const next = { ...search };
              if (key === null) delete next.peekIssueId;
              else next.peekIssueId = key;
              void navigate({ to: ".", search: next, replace: true } as never);
            }}
            compact={search.layout === "compact"}
          />
        </React.Suspense>
      </>
    );
  },
});
