// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One analytics tab (parity/specs/analytics.md §§1, 4, ANL-001,
// ANL-004). The address names the tab; an unknown id renders the
// shell with no panel instead of stranding the page. The picker
// scope rides a validated project_ids search parameter that tab
// switches carry along. Thin: the loader prefetches the project
// list the shell decides on, while the totals read stays an
// on-mount read inside the screen (spec §16.6).

import { createFileRoute } from "@tanstack/react-router";
import { Button, Skeleton } from "@pidash/kit";
import * as React from "react";

import { AnalyticsScreen } from "../../../../features/analytics/index.js";
// Search helpers import deep on purpose: the shell (loader,
// validateSearch, pending view) stays in the initial chain, and a
// barrel import here would drag the whole feature back into it
// (NEWFRONT-732). The screen import above belongs to the split
// component chunk.
import { analyticsScopeSearch, parseAnalyticsSearch } from "../../../../features/analytics/lib/search.js";
import { projectsQueryOptions } from "../../../../features/projects/index.js";

export const Route = createFileRoute("/$ws/_gated/analytics/$tabId")({
  validateSearch: (search: Record<string, unknown>) => parseAnalyticsSearch(search),
  loader: async ({ context, params }) => {
    await context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws));
  },
  pendingComponent: AnalyticsRoutePending,
  errorComponent: AnalyticsRouteError,
  component: AnalyticsTabRoute,
});

function AnalyticsRoutePending(): React.ReactElement {
  return (
    <div data-testid="analytics-route-pending" className="flex flex-col gap-(--space-3) p-(--space-6)">
      <Skeleton height="40px" />
      <Skeleton height="120px" />
    </div>
  );
}

function AnalyticsRouteError({ error, reset }: { error: unknown; reset: () => void }): React.ReactElement {
  return (
    <section
      aria-label="Analytics"
      data-testid="analytics-route-error"
      className="flex flex-col items-center gap-(--space-4) p-(--space-12)"
    >
      <h2 className="text-h3 text-(--text)">Could not load analytics</h2>
      <p className="text-body text-(--text-muted)">
        {error instanceof Error ? error.message : "Something went wrong."}
      </p>
      <Button variant="primary" onClick={reset}>
        Try again
      </Button>
    </section>
  );
}

function AnalyticsTabRoute(): React.ReactElement {
  const { ws, tabId } = Route.useParams();
  const search = Route.useSearch();
  const navigate = Route.useNavigate();
  const projectIds = search.project_ids ?? [];
  const handleScopeChange = (ids: string[]): void => {
    void navigate({ to: ".", search: analyticsScopeSearch(ids), replace: true });
  };
  return (
    <AnalyticsScreen
      key={ws}
      workspaceSlug={ws}
      tabId={tabId}
      projectIds={projectIds}
      onScopeChange={handleScopeChange}
    />
  );
}
