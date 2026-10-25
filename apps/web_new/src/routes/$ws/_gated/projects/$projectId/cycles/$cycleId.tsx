// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle detail route (parity/specs/cycles.md §13, CYC-032–034; §14,
// CYC-035). The route definition is eager in the router tree, so it keeps
// only search validation, prefetching and the gone-away gate; the page body
// loads with its own chunk (size gate).

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { isApiError } from "@pidash/api-client";
import { Button } from "@pidash/kit";
import * as React from "react";

import { CycleDetailGate } from "../../../../../../features/cycles/components/CycleDetailGate.js";
import { cycleDetailQueryOptions } from "../../../../../../features/cycles/api/cycleDetail.js";
import { loadIssueListLeaves, primeIssueListLeaves } from "../../../../../../features/issues/components/lazyLeaves.js";
import { loadKanbanLayout } from "../../../../../../features/issues/kanban/lazy.js";
import { ListSkeleton } from "../../../../../../features/issues/loaders/ListSkeleton.js";
import { parseIssueSearch } from "../../../../../../features/issues/filters/search.js";
import { preloadIssueFilters } from "../../../../../../features/issues/filters/rich/lazy.js";
import { projectsQueryOptions } from "../../../../../../features/projects/api/queries.js";

// Start the leaf and filter chunks with the route module, beside its data
// reads, so the first layout render does not suspend and mount twice
// (NEWFRONT-619, NEWFRONT-669). These loaders are tiny; the chunks they
// request stay off the initial chain.
primeIssueListLeaves();
preloadIssueFilters();

const loadCycleDetailContent = () => import("./CycleDetailContent.js");
const LazyCycleDetailContent = React.lazy(() =>
  loadCycleDetailContent().then((module) => ({ default: module.CycleDetailContent }))
);

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/cycles/$cycleId")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  beforeLoad: async ({ context, params }) => {
    const { prefetchEntityList } = await import("../../../../../../features/issues/list/prefetch.js");
    const entity = { scope: "cycle", projectId: params.projectId, cycleId: params.cycleId } as const;
    try {
      // Preferences first: the stored flags shape the first page read.
      await prefetchEntityList(context, params.ws, entity);
      await context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws));
      await context.queryClient.ensureQueryData(
        cycleDetailQueryOptions(context.apiClient, params.ws, params.projectId, params.cycleId)
      );
      // The page body, the board and the list/analytics leaves ride their
      // own chunks; await them with the data so the first commit is the
      // screen rather than an intermediate skeleton.
      await Promise.all([loadCycleDetailContent(), loadKanbanLayout(), loadIssueListLeaves()]);
    } catch (error) {
      // A missing cycle 404s the prefetch: let the gone-away gate own it
      // instead of failing the load (CYC-013).
      if (!isApiError(error) || error.status !== 404) throw error;
    }
  },
  pendingComponent: function CycleIssuesPending() {
    return <ListSkeleton />;
  },
  errorComponent: function CycleDetailError({ error, reset }: { error: unknown; reset: () => void }) {
    return (
      <section aria-label="Cycle details" className="flex flex-col items-center gap-(--space-4) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">Could not load cycle details</h2>
        <p className="text-body text-(--text-muted)">
          {error instanceof Error ? error.message : "Something went wrong."}
        </p>
        <Button variant="primary" onClick={reset}>
          Try again
        </Button>
      </section>
    );
  },
  component: function CycleDetailRoute() {
    const params = Route.useParams();
    const search = Route.useSearch();
    const navigate = useNavigate();
    const setPeekKey = (key: string | null): void => {
      const next = { ...search };
      if (key === null) delete next.peekIssueId;
      else next.peekIssueId = key;
      void navigate({ to: ".", search: next, replace: true } as never);
    };
    return (
      <CycleDetailGate workspaceSlug={params.ws} projectId={params.projectId} cycleId={params.cycleId}>
        <React.Suspense fallback={<ListSkeleton />}>
          <LazyCycleDetailContent
            workspaceSlug={params.ws}
            projectId={params.projectId}
            cycleId={params.cycleId}
            peekKey={search.peekIssueId ?? null}
            onPeekChange={setPeekKey}
            compact={search.layout === "compact"}
          />
        </React.Suspense>
      </CycleDetailGate>
    );
  },
});
