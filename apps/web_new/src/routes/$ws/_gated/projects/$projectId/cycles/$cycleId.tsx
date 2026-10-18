// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle detail host for the issues list (parity/specs/issues.md §2,
// ISS-007–014). The Cycles area owns the full page; this slice proves
// cycle-context grouping, group-header create/attach and the read-only
// completed state here by mounting the shared list layout.

import { useQuery } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../../../../core/api/client.js";
import { ListSkeleton, parseIssueSearch, prefsQueryOptions } from "../../../../../../features/issues/index.js";
import { EntityListScreen } from "../../../../../../features/issues/list/EntityListScreen.js";
import { loadListLayout } from "../../../../../../features/issues/list/lazy.js";
import { listPageQueryOptions } from "../../../../../../features/issues/list/pages.js";
import { isCompletedCycle, pickerCyclesQueryOptions } from "../../../../../../shared/pickers/index.js";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/cycles/$cycleId")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  beforeLoad: async ({ context, params }) => {
    const entity = { scope: "cycle", projectId: params.projectId, cycleId: params.cycleId } as const;
    await Promise.all([
      loadListLayout(),
      context.queryClient.ensureQueryData(
        listPageQueryOptions(context.apiClient, params.ws, { entity, groupBy: null, orderBy: "sort_order" })
      ),
      context.queryClient.ensureQueryData(prefsQueryOptions(context.apiClient, params.ws, entity)),
    ]);
  },
  pendingComponent: function CycleIssuesPending() {
    return <ListSkeleton />;
  },
  component: function CycleIssuesScreen() {
    const params = Route.useParams();
    const search = Route.useSearch();
    const navigate = useNavigate();
    const { data: cycles } = useQuery(pickerCyclesQueryOptions(getAppClient(), params.ws, params.projectId));
    const cycle = cycles?.find((row) => row.id === params.cycleId) ?? null;
    const entity = { scope: "cycle", projectId: params.projectId, cycleId: params.cycleId } as const;
    return (
      <EntityListScreen
        workspaceSlug={params.ws}
        entity={entity}
        projectId={params.projectId}
        title={cycle?.name ?? "Cycle"}
        cycleId={params.cycleId}
        completedCycle={isCompletedCycle(cycle?.status)}
        peekKey={search.peekIssueId ?? null}
        onPeekChange={(key) => {
          const next = { ...search };
          if (key === null) delete next.peekIssueId;
          else next.peekIssueId = key;
          void navigate({ to: ".", search: next, replace: true } as never);
        }}
        compact={search.layout === "compact"}
      />
    );
  },
});
