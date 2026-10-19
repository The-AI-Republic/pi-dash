// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module detail host for the issues list (parity/specs/issues.md §2,
// ISS-007–014). The Modules area owns the full page; this slice mounts
// the shared list layout so module-context grouping and group-header
// create/attach work there too.

import { useQuery } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../../../../core/api/client.js";
import { ListSkeleton, parseIssueSearch, prefsQueryOptions } from "../../../../../../features/issues/index.js";
import { loadListLayout } from "../../../../../../features/issues/list/lazy.js";
import { pickerModulesQueryOptions } from "../../../../../../shared/pickers/index.js";

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
    const { data: modules } = useQuery(pickerModulesQueryOptions(getAppClient(), params.ws, params.projectId));
    const module = modules?.find((row) => row.id === params.moduleId) ?? null;
    const entity = { scope: "module", projectId: params.projectId, moduleId: params.moduleId } as const;
    return (
      <React.Suspense fallback={<ListSkeleton />}>
        <LazyEntityListScreen
          workspaceSlug={params.ws}
          entity={entity}
          projectId={params.projectId}
          title={module?.name ?? "Module"}
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
