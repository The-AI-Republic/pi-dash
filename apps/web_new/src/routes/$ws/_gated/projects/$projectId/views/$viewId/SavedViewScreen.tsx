// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved project view body (parity/specs/issues.md §21, ISS-104). The route
// module is part of the eager tree (the router imports every route file),
// so the view screen lives here and is requested on first visit: it reads
// the view record, applies its stored expression to the project list,
// renders the layouts the issues screen offers (driven by the view's own
// stored display settings, not the personal ones) and hands the record to
// the filter row so Save as and Update view act on it. The views area owns
// the full views surface and replaces this host when it lands (NEWFRONT-677,
// Quality gates: the stored-expression, display-preference and filter-row
// modules are not part of the shell's first paint).

import { useSuspenseQuery, type QueryClient } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";
import * as React from "react";

import {
  DisplayControl,
  IssuesLayoutRoot,
  ListSkeleton,
  issuesQueryOptions,
  mergeDisplayPrefs,
  parseDisplayPrefs,
  selectCreatableProjects,
  type LayoutKind,
  type WorkItemCreateProject,
} from "../../../../../../../features/issues/index.js";
import { SavedViewIssuesEmpty } from "../../../../../../../features/issues/emptyStates/cards.js";
import type { RichFilterBuilderParts } from "../../../../../../../features/issues/filters/rich/RichFilterBuilder.js";
import { projectViewQueryOptions } from "../../../../../../../features/issues/filters/rich/savedViewQuery.js";
import { savedViewKeys, viewExpression } from "../../../../../../../features/issues/filters/rich/views.js";
import {
  ResolvedAnalyticsEntry,
  ResolvedIssueList,
} from "../../../../../../../features/issues/components/lazyLeaves.js";
import { projectsQueryOptions } from "../../../../../../../features/projects/index.js";
import { canEditInProject } from "../../../../../../../shared/shell/contextual/index.js";
import type { SavedViewReadInput } from "../../../../../../../features/issues/filters/rich/savedViewPreload.js";

// The route's `beforeLoad` runs this beside the screen's own request, so the
// reads and the screen it feeds arrive in one chunk.
export { prefetchProjectView as prefetchSavedView } from "../../../../../../../features/issues/filters/rich/savedViewPreload.js";

const LazyIssueFilters = React.lazy(() => import("../../../../../../../features/issues/filters/rich/IssueFilters.js"));

// The timeline boundary only matters when the view stores the timeline
// layout, so it rides its own chunk (Quality gates, NEWFRONT-677).
// The create dialog carries the property strip and every picker; a view
// opens it from its empty card, so it stays off this screen's first paint.
const LazyWorkItemCreateDialog = React.lazy(() =>
  import("../../../../../../../features/issues/components/WorkItemCreateDialog.js").then((module) => ({
    default: module.WorkItemCreateDialog,
  }))
);

const LazyGanttTimeline = React.lazy(() =>
  import("../../../../../../../features/issues/gantt/GanttTimeline.js").then((module) => ({
    default: module.GanttTimeline,
  }))
);

export interface SavedViewScreenProps {
  params: SavedViewReadInput;
  /** Validated route search, exactly as the issues screen reads it. */
  search: { layout?: "list" | "compact" | undefined };
  context: { apiClient: ApiClient; queryClient: QueryClient };
}

export function SavedViewScreen({ params, search, context }: SavedViewScreenProps): React.ReactElement {
  const { data: view } = useSuspenseQuery(
    projectViewQueryOptions(context.apiClient, params.ws, params.projectId, params.viewId)
  );
  const { data: projects } = useSuspenseQuery(projectsQueryOptions(context.apiClient, params.ws));
  const project = projects.find((entry) => entry.id === params.projectId) ?? null;
  const [createOpen, setCreateOpen] = React.useState(false);
  const creatable: WorkItemCreateProject[] = React.useMemo(
    () =>
      selectCreatableProjects(
        projects.map((entry) => ({
          id: entry.id,
          name: entry.name,
          identifier: entry.identifier,
          memberRole: entry.member_role,
          archived: entry.archived_at !== null,
        }))
      ),
    [projects]
  );
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
  // The card's entry follows the project standing, like the project list:
  // a guest on this project sees it dead even when other projects take
  // their new work items.
  const canEdit = canEditInProject({ memberRole: project.member_role, archived: project.archived_at !== null });
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
    <ResolvedAnalyticsEntry
      workspaceSlug={params.ws}
      projectId={params.projectId}
      projectName={project.name}
      rows={issues.results}
      totalCount={issues.total_count}
    />
  );
  const renderWithFilters = (richFilter: RichFilterBuilderParts | null): React.ReactElement => {
    // ISS-072 pins one card here regardless of the expression: a view
    // that matches nothing states what the view is for and offers the
    // create dialog, rather than reading as a search that failed.
    const emptyState = (
      <SavedViewIssuesEmpty
        canAct={canEdit}
        onCreate={() => {
          setCreateOpen(true);
        }}
      />
    );
    return (
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
            <React.Suspense fallback={<ListSkeleton />}>
              <LazyGanttTimeline
                workspaceSlug={params.ws}
                projectId={params.projectId}
                projectIdentifier={project.identifier}
                issues={issues.results}
                totalCount={issues.total_count}
                prefs={prefs}
              />
            </React.Suspense>
          ) : (
            <ResolvedIssueList
              issues={issues.results}
              projectIdentifier={project.identifier}
              layout={search.layout ?? "list"}
            />
          )
        }
        emptyState={emptyState}
        noMatchState={emptyState}
        headerActions={displayControl}
        displayControl={displayControl}
        filterControl={richFilter?.control}
        filterRow={richFilter?.row}
        analyticsControl={analytics}
      />
    );
  };
  const createDialog = createOpen ? (
    <React.Suspense fallback={null}>
      <LazyWorkItemCreateDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        workspaceSlug={params.ws}
        projects={creatable}
        initialProjectId={params.projectId}
      />
    </React.Suspense>
  ) : null;
  return (
    <>
      <React.Suspense fallback={renderWithFilters(null)}>
        <LazyIssueFilters
          workspaceSlug={params.ws}
          projectId={params.projectId}
          entity={{ scope: "project", projectId: params.projectId }}
          hideCycle={!project.cycle_view}
          hideModule={!project.module_view}
          viewsEnabled={project.issue_views_view}
          view={view}
          viewDisplay={viewDisplay}
          onViewUpdated={(stored) => {
            context.queryClient.setQueryData(savedViewKeys.project(params.ws, params.projectId, params.viewId), stored);
          }}
        >
          {renderWithFilters}
        </LazyIssueFilters>
      </React.Suspense>
      {createDialog}
    </>
  );
}
