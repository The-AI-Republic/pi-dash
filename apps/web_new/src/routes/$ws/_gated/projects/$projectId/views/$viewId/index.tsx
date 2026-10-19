// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved project view (parity/specs/issues.md §21, ISS-104). This is the
// minimal issues-owned list a saved view opens into: it reads the view
// record, applies its stored expression to the project list, renders the
// layouts the issues screen offers (driven by the view's own stored display
// settings, not the personal ones) and hands the record to the filter row
// so Save as and Update view act on it. The views area owns the full views
// surface and replaces this host when it lands.

import { useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { Button } from "@pidash/kit";
import * as React from "react";

import { isProjectGateError } from "../../../../../../../features/auth/index.js";
import {
  DisplayControl,
  IssuesLayoutRoot,
  ListSkeleton,
  issuesQueryOptions,
  parseIssueSearch,
  parseDisplayPrefs,
  mergeDisplayPrefs,
  type LayoutKind,
} from "../../../../../../../features/issues/index.js";
import type { RichFilterBuilderParts } from "../../../../../../../features/issues/filters/rich/RichFilterBuilder.js";
import { GanttTimeline } from "../../../../../../../features/issues/gantt/GanttTimeline.js";
import { orderByFromDisplayFilters } from "../../../../../../../features/issues/prefs/prefsQuery.js";
import { projectViewQueryOptions } from "../../../../../../../features/issues/filters/rich/savedViewQuery.js";
import { viewExpression } from "../../../../../../../features/issues/filters/rich/views.js";
import { projectsQueryOptions } from "../../../../../../../features/projects/index.js";

// First paint loads the header and the layout shell only. The list pulls in
// the row virtualizer, and the analytics control opens a dialog, so neither
// is needed until the screen mounts; each gets its own chunk, keeping the
// route inside the initial-JS budget (Quality gates).
const LazyIssueList = React.lazy(() =>
  import("../../../../../../../features/issues/components/IssueList.js").then((module) => ({
    default: module.IssueList,
  }))
);
const LazyAnalyticsEntry = React.lazy(() =>
  import("../../../../../../../features/issues/analytics/AnalyticsEntry.js").then((module) => ({
    default: module.AnalyticsEntry,
  }))
);
const LazyIssueFilters = React.lazy(() => import("../../../../../../../features/issues/filters/rich/IssueFilters.js"));

function ViewIssuesEmptyState(): React.ReactElement {
  return (
    <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
      <h2 className="text-h3 text-(--text)">No issues yet</h2>
      <p className="text-body text-(--text-muted)">New issues in this project will appear here.</p>
    </section>
  );
}

function ViewIssuesNoMatchState(): React.ReactElement {
  return (
    <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
      <h2 className="text-h3 text-(--text)">No matching issues</h2>
      <p className="text-body text-(--text-muted)">No issues match the current filters.</p>
    </section>
  );
}

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/views/$viewId/")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  beforeLoad: async ({ context, params, search }) => {
    try {
      const view = await context.queryClient.ensureQueryData(
        projectViewQueryOptions(context.apiClient, params.ws, params.projectId, params.viewId)
      );
      // The view carries its own order, so the list query never reads the
      // personal preferences for this screen.
      await context.queryClient.ensureQueryData(
        issuesQueryOptions(
          context.apiClient,
          params.ws,
          params.projectId,
          search,
          viewExpression(view),
          orderByFromDisplayFilters(view.display_filters)
        )
      );
    } catch (error) {
      if (!isProjectGateError(error)) throw error;
    }
  },
  loader: async ({ context, params }) => {
    await context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws));
  },
  pendingComponent: function ViewIssuesPending() {
    return <ListSkeleton />;
  },
  errorComponent: function ViewIssuesError({ error, reset }: { error: unknown; reset: () => void }) {
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
  component: function SavedViewScreen() {
    const params = Route.useParams();
    const search = Route.useSearch();
    const context = Route.useRouteContext();

    const { data: view } = useSuspenseQuery(
      projectViewQueryOptions(context.apiClient, params.ws, params.projectId, params.viewId)
    );
    const { data: projects } = useSuspenseQuery(projectsQueryOptions(context.apiClient, params.ws));
    const project = projects.find((entry) => entry.id === params.projectId) ?? null;
    // The layout root renders from these settings rather than the personal
    // record, so the view opens in the layout it was saved with. Display
    // switches edit this copy; Update view persists it.
    const storedDisplay = React.useMemo(
      () => parseDisplayPrefs(view.display_filters, view.display_properties),
      [view.display_filters, view.display_properties]
    );
    const [viewDisplay, setViewDisplay] = React.useState(storedDisplay);
    React.useEffect(() => {
      setViewDisplay(storedDisplay);
    }, [storedDisplay]);
    const { data: issues } = useSuspenseQuery(
      issuesQueryOptions(
        context.apiClient,
        params.ws,
        params.projectId,
        search,
        viewExpression(view),
        viewDisplay.orderBy
      )
    );

    if (project === null) {
      return (
        <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
          <h2 className="text-h3 text-(--text)">Project not found</h2>
          <p className="text-body text-(--text-muted)">Pick another project from the sidebar.</p>
        </section>
      );
    }

    const offered: readonly LayoutKind[] = ["list", "gantt_chart"];
    const displayControl = (
      <DisplayControl
        workspaceSlug={params.ws}
        entity={{ scope: "project", projectId: params.projectId }}
        offered={offered}
        hideCycle={!project.cycle_view}
        hideModule={!project.module_view}
        display={viewDisplay}
        onDisplayChange={setViewDisplay}
      />
    );
    const analytics = (
      <LazyAnalyticsEntry
        workspaceSlug={params.ws}
        projectId={params.projectId}
        projectName={project.name}
        rows={issues.results}
        totalCount={issues.total_count}
      />
    );
    const renderWithFilters = (richFilter: RichFilterBuilderParts | null): React.ReactElement => (
      <IssuesLayoutRoot
        workspaceSlug={params.ws}
        entity={{ scope: "project", projectId: params.projectId }}
        title={view.name}
        totalCount={issues.total_count}
        resultCount={issues.results.length}
        offered={offered}
        display={viewDisplay}
        onDisplayChange={(layout) => setViewDisplay((current) => mergeDisplayPrefs(current, { layout }))}
        renderLayout={(layout, prefs) =>
          layout === "gantt_chart" ? (
            <GanttTimeline
              workspaceSlug={params.ws}
              projectId={params.projectId}
              projectIdentifier={project.identifier}
              issues={issues.results}
              totalCount={issues.total_count}
              prefs={prefs}
            />
          ) : (
            <React.Suspense fallback={<ListSkeleton />}>
              <LazyIssueList
                issues={issues.results}
                projectIdentifier={project.identifier}
                layout={search.layout ?? "list"}
              />
            </React.Suspense>
          )
        }
        emptyState={<ViewIssuesEmptyState />}
        noMatchState={<ViewIssuesNoMatchState />}
        headerActions={displayControl}
        displayControl={displayControl}
        filterControl={richFilter?.control}
        filterRow={richFilter?.row}
        analyticsControl={<React.Suspense fallback={null}>{analytics}</React.Suspense>}
      />
    );
    return (
      <React.Suspense fallback={renderWithFilters(null)}>
        <LazyIssueFilters
          workspaceSlug={params.ws}
          projectId={params.projectId}
          hideCycle={!project.cycle_view}
          hideModule={!project.module_view}
          viewsEnabled={project.issue_views_view}
          view={view}
          viewDisplay={viewDisplay}
          onViewUpdated={(stored) => {
            context.queryClient.setQueryData(
              projectViewQueryOptions(context.apiClient, params.ws, params.projectId, params.viewId).queryKey,
              stored
            );
          }}
        >
          {renderWithFilters}
        </LazyIssueFilters>
      </React.Suspense>
    );
  },
});
