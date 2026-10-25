// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project issues (parity/specs/issues.md §1 and §9, ISS-001–006 and
// ISS-075–081/ISS-106). The generated route tree imports this definition
// eagerly, so the screen body lives in IssuesListContent and `beforeLoad`
// starts that chunk beside the data. The first render still gets the screen,
// while the measured initial closure keeps only the route definition.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { Button } from "@pidash/kit";
import * as React from "react";

import { isProjectGateError } from "../../../../../../features/auth/denials.js";
import { ListSkeleton } from "../../../../../../features/issues/loaders/ListSkeleton.js";
import { issuesQueryOptions } from "../../../../../../features/issues/api/queries.js";
import { parseIssueSearch } from "../../../../../../features/issues/filters/search.js";
import { prefsQueryOptions } from "../../../../../../features/issues/prefs/prefsQuery.js";
import { loadProjectEmptyState } from "../../../../../../features/issues/emptyStates/lazy.js";
import { loadIssueListLeaves, primeIssueListLeaves } from "../../../../../../features/issues/components/lazyLeaves.js";
import { loadListLayout } from "../../../../../../features/issues/list/lazy.js";
import { loadKanbanLayout } from "../../../../../../features/issues/kanban/lazy.js";
import { preloadIssueFilters } from "../../../../../../features/issues/filters/rich/lazy.js";
import { orderByOf } from "../../../../../../features/issues/prefs/prefsQuery.js";
import { projectsQueryOptions } from "../../../../../../features/projects/api/queries.js";

// Start the leaf and filter chunks with the route module, beside its data
// reads, so the first layout render does not suspend and mount twice
// (NEWFRONT-619, NEWFRONT-669).
primeIssueListLeaves();
preloadIssueFilters();

const loadIssuesListContent = () => import("./IssuesListContent.js");
const LazyIssuesListContent = React.lazy(() =>
  loadIssuesListContent().then((module) => ({ default: module.IssuesListContent }))
);

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/issues/")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  beforeLoad: async ({ context, params, search }) => {
    try {
      // Preferences first: the stored order is part of the list query, so
      // the timeline and the list both ask for the order the user chose.
      const record = await context.queryClient.ensureQueryData(
        prefsQueryOptions(context.apiClient, params.ws, { scope: "project", projectId: params.projectId })
      );
      await context.queryClient.ensureQueryData(
        issuesQueryOptions(
          context.apiClient,
          params.ws,
          params.projectId,
          search,
          record.rich_filters,
          orderByOf(record)
        )
      );
      // Start the screen with the data and await the same leaves it uses, so
      // the first commit is the screen rather than an intermediate skeleton.
      await Promise.all([
        loadIssuesListContent(),
        loadIssueListLeaves(),
        loadListLayout(),
        loadProjectEmptyState(),
        loadKanbanLayout(),
      ]);
    } catch (error) {
      // Denial statuses belong to the project gate (AUTH-024): let them
      // through so the gate renders the denial screen instead of the load
      // error.
      if (!isProjectGateError(error)) throw error;
    }
  },
  loader: async ({ context, params }) => {
    await context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws));
  },
  // The stored layout is unknown while the prefetch runs (it is fetched,
  // not in the URL), so the pending window always shows the list
  // placeholder; the layout renders its own skeleton once mounted.
  pendingComponent: function IssuesPending() {
    return <ListSkeleton />;
  },
  errorComponent: function IssueListError({ error, reset }: { error: unknown; reset: () => void }) {
    return (
      <section aria-label="Issues" className="flex flex-col items-center gap-(--space-4) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">Could not load issues</h2>
        <p className="text-body text-(--text-muted)">
          {error instanceof Error ? error.message : "Something went wrong."}
        </p>
        <Button variant="primary" onClick={reset}>
          Try again
        </Button>
      </section>
    );
  },
  component: function IssueListScreen() {
    const params = Route.useParams();
    const search = Route.useSearch();
    const context = Route.useRouteContext();
    const navigate = useNavigate();
    return (
      <React.Suspense fallback={<ListSkeleton />}>
        <LazyIssuesListContent
          workspaceSlug={params.ws}
          projectId={params.projectId}
          search={search}
          apiClient={context.apiClient}
          queryClient={context.queryClient}
          navigate={navigate}
        />
      </React.Suspense>
    );
  },
});
