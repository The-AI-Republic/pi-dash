// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module detail host for the issues list (parity/specs/issues.md §2,
// ISS-007–014). The Modules area owns the full page; this slice mounts
// the shared list layout so module-context grouping and group-header
// create/attach work there too.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

// Direct module imports: this route definition is eager, and the feature
// barrel would pull the analytics entry and picker barst into the initial
// bundle (NEWFRONT-619, size gate).
import { parseIssueSearch } from "../../../../../../features/issues/filters/search.js";
import { ListSkeleton } from "../../../../../../features/issues/loaders/ListSkeleton.js";
import { prefsQueryOptions } from "../../../../../../features/issues/prefs/prefsQuery.js";

// The list host and its paging module are part of the list layout chunk;
// importing them statically would pull them into the entry chunk through
// the generated route tree (Quality gates).
const LazyEntityListScreen = React.lazy(() =>
  import("../../../../../../features/issues/list/EntityListScreen.js").then((module) => ({
    default: module.EntityListScreen,
  }))
);

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/modules/$moduleId")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  beforeLoad: async ({ context, params }) => {
    const { loadListLayout } = await import("../../../../../../features/issues/list/lazy.js");
    const { listPageQueryOptions } = await import("../../../../../../features/issues/list/pages.js");
    const { selectDisplayPrefs } = await import("../../../../../../features/issues/prefs/displayPrefs.js");
    const entity = { scope: "module", projectId: params.projectId, moduleId: params.moduleId } as const;
    const record = await context.queryClient.ensureQueryData(prefsQueryOptions(context.apiClient, params.ws, entity));
    const showSubIssues = selectDisplayPrefs(record).showSubIssues;
    await Promise.all([
      loadListLayout(),
      context.queryClient.ensureQueryData(
        listPageQueryOptions(context.apiClient, params.ws, {
          entity,
          groupBy: null,
          orderBy: "sort_order",
          showSubIssues,
        })
      ),
    ]);
  },
  pendingComponent: function ModuleIssuesPending() {
    return <ListSkeleton />;
  },
  component: function ModuleIssuesScreen() {
    const params = Route.useParams();
    const search = Route.useSearch();
    const navigate = useNavigate();
    const entity = { scope: "module", projectId: params.projectId, moduleId: params.moduleId } as const;
    return (
      <React.Suspense fallback={<ListSkeleton />}>
        <LazyEntityListScreen
          workspaceSlug={params.ws}
          entity={entity}
          projectId={params.projectId}
          moduleId={params.moduleId}
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
    );
  },
});
