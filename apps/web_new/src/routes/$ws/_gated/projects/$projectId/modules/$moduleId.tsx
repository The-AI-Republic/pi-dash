// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module detail (parity/specs/modules.md §8, MOD-029, MOD-030, MOD-032).
// The Modules area owns the full page: the gone-away gate, then the
// detail screen with its trail, shared work-item layouts and analytics
// sidebar. The route stays thin — search validation, data and chunk
// prefetch, layout — with the composition in the feature.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { Button } from "@pidash/kit";
import * as React from "react";

// Direct module imports: this route definition is eager, and the feature
// barrels would pull the list body, dialogs and picker parts into the
// initial bundle (NEWFRONT-619, size gate).
import { isProjectGateError } from "../../../../../../features/auth/index.js";
import { loadKanbanLayout } from "../../../../../../features/issues/kanban/lazy.js";
import { loadIssueListLeaves, primeIssueListLeaves } from "../../../../../../features/issues/components/lazyLeaves.js";
import { ListSkeleton } from "../../../../../../features/issues/loaders/ListSkeleton.js";
import { parseIssueSearch } from "../../../../../../features/issues/filters/search.js";
import { preloadIssueFilters } from "../../../../../../features/issues/filters/rich/lazy.js";
import { peekSearchFor, stripPeekParams, type PeekRequest } from "../../../../../../features/issues/peek/peekUrl.js";
import { projectsQueryOptions } from "../../../../../../features/projects/api/queries.js";

// Start the leaf and filter chunks with the route module so the first
// layout render does not suspend and mount twice (NEWFRONT-619).
primeIssueListLeaves();
preloadIssueFilters();

// The screen carries the entity host, the sidebar and the pickers; it
// rides its own chunk behind the route's data prefetch (Quality gates).
const LazyModuleDetailScreen = React.lazy(() =>
  import("../../../../../../features/modules/components/ModuleDetailScreen.js").then((module) => ({
    default: module.ModuleDetailScreen,
  }))
);
// The gate reads the module through the modules sidebar queries, which
// take the issues feature through its barrel. Await both chunks in the
// route load so neither screen code nor the issue list joins the entry.
const loadModuleDetailGate = () =>
  import("../../../../../../features/modules/components/ModuleDetailGate.js").then((module) => ({
    default: module.ModuleDetailGate,
  }));
const LazyModuleDetailGate = React.lazy(loadModuleDetailGate);

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/modules/$moduleId")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  beforeLoad: async ({ context, params }) => {
    const [{ prefetchEntityList }, { moduleDetailQueryOptions }] = await Promise.all([
      import("../../../../../../features/issues/list/prefetch.js"),
      import("../../../../../../features/modules/components/sidebar/moduleSidebarQueries.js"),
    ]);
    const entity = { scope: "module", projectId: params.projectId, moduleId: params.moduleId } as const;
    try {
      // Preferences first: the stored flags shape the first page read.
      await prefetchEntityList(context, params.ws, entity);
      await context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws));
      await context.queryClient.ensureQueryData(
        moduleDetailQueryOptions(context.apiClient, params.ws, params.projectId, params.moduleId)
      );
      // The board and the list/analytics leaves ride their own chunks;
      // await them with the data so the first commit is the screen.
      await Promise.all([loadKanbanLayout(), loadIssueListLeaves(), loadModuleDetailGate()]);
    } catch (error) {
      // A missing module 404s the prefetch, and a reader without project
      // access is refused with 403/409: both belong to a gate, not to the
      // route's error screen. The module gate splits them — missing state
      // for the id, the shared project access wall for the refusal
      // (MOD-029, MOD-038) — so the load completes and that gate renders.
      if (!isProjectGateError(error)) throw error;
    }
  },
  pendingComponent: function ModuleDetailPending() {
    return <ListSkeleton />;
  },
  errorComponent: function ModuleDetailError({ error, reset }: { error: unknown; reset: () => void }) {
    return (
      <section aria-label="Module details" className="flex flex-col items-center gap-(--space-4) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">Could not load module details</h2>
        <p className="text-body text-(--text-muted)">
          {error instanceof Error ? error.message : "Something went wrong."}
        </p>
        <Button variant="primary" onClick={reset}>
          Try again
        </Button>
      </section>
    );
  },
  component: function ModuleDetailRoute() {
    const params = Route.useParams();
    const search = Route.useSearch();
    const navigate = useNavigate();
    const setPeek = (request: PeekRequest): void => {
      // Closing strips every peek param, nesting included; opening writes
      // the child's project when it differs from the route (ISS-167).
      const next = request === null ? stripPeekParams({ ...search }) : peekSearchFor(search, request, params.projectId);
      void navigate({ to: ".", search: next, replace: true } as never);
    };
    return (
      <React.Suspense fallback={<ListSkeleton />}>
        <LazyModuleDetailGate workspaceSlug={params.ws} projectId={params.projectId} moduleId={params.moduleId}>
          <React.Suspense fallback={<ListSkeleton />}>
            <LazyModuleDetailScreen
              workspaceSlug={params.ws}
              projectId={params.projectId}
              moduleId={params.moduleId}
              peekKey={search.peekIssueId ?? null}
              onPeekChange={setPeek}
              peekProjectId={search.peekProjectId ?? null}
              compact={search.layout === "compact"}
            />
          </React.Suspense>
        </LazyModuleDetailGate>
      </React.Suspense>
    );
  },
});
