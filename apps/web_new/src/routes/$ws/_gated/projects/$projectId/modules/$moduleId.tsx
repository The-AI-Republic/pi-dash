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
// barrel would pull the analytics entry and picker parts into the initial
// bundle (NEWFRONT-619, size gate).
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

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/modules/$moduleId")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  beforeLoad: async ({ context, params }) => {
    const { prefetchEntityList } = await import("../../../../../../features/issues/list/prefetch.js");
    const entity = { scope: "module", projectId: params.projectId, moduleId: params.moduleId } as const;
    await prefetchEntityList(context, params.ws, entity);
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
