// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project issues (parity/specs/issues.md §1 and §9, ISS-001–006 and
// ISS-075–081/ISS-106). Thin: the single search param validated by zod,
// a loader that prefetches display preferences and then the issue list in
// the user's stored order, and the per-entity layout root with the
// Display control in both headers. Layout slices extend the offered list
// with their renderer; the timeline is a lazy leaf so its chart code stays
// out of the route chunk. The empty cards follow ISS-068: the first-work-
// item prompt until the filter row adds a condition during this visit, and
// the no-results card with clear-all from then on.

import { useSuspenseQuery, type QueryClient } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";
import { Button } from "@pidash/kit";
import * as React from "react";

import { invalidateWorkspace } from "../../../../../../core/query/optimistic.js";
import { DisplayControl } from "../../../../../../features/issues/display/DisplayControl.js";
import { IssuesLayoutRoot } from "../../../../../../features/issues/layouts/IssuesLayoutRoot.js";
import { resolveActiveLayout, type LayoutKind } from "../../../../../../features/issues/layouts/kinds.js";
import { ListSkeleton } from "../../../../../../features/issues/loaders/ListSkeleton.js";
import { issuesQueryOptions } from "../../../../../../features/issues/api/queries.js";
import type { IssueSearch } from "../../../../../../features/issues/filters/search.js";
import { prefsQueryOptions } from "../../../../../../features/issues/prefs/prefsQuery.js";
import { useWeekStart } from "../../../../../../features/issues/dropdowns/weekStart.js";
import { selectCreatableProjects } from "../../../../../../features/issues/create/createForm.js";
import type { WorkItemCreateProject } from "../../../../../../features/issues/components/WorkItemCreateDialog.js";
import type { RichFilterBuilderParts } from "../../../../../../features/issues/filters/rich/RichFilterBuilder.js";
import { ProjectIssuesEmptyState } from "../../../../../../features/issues/emptyStates/lazy.js";
import { ResolvedAnalyticsEntry } from "../../../../../../features/issues/components/lazyLeaves.js";
import { resolvedListLayout } from "../../../../../../features/issues/list/lazy.js";
import { resolvedKanbanLayout } from "../../../../../../features/issues/kanban/lazy.js";
// Keep the calendar off the issues route chunk: its entry carries the
// calendar skeleton, and first paint should stay inside the size gate.
const LazyCalendarEntry = React.lazy(() =>
  import("../../../../../../features/issues/calendar/CalendarEntry.js").then((module) => ({
    default: module.CalendarEntry,
  }))
);
// The empty cards ride their own chunk; the route awaits it with the list
// leaves in `beforeLoad` so the layout mounts once (Quality gates).
import { orderByOf } from "../../../../../../features/issues/prefs/prefsQuery.js";
import type { PeekRequest } from "../../../../../../features/issues/peek/peekUrl.js";
import { canEditInProject } from "../../../../../../shared/shell/contextual/index.js";

// The list layout is a separate chunk so the route stays inside the
// initial-JS budget (Quality gates). `beforeLoad` awaits it with the data,
// so the component mounts once instead of suspending after commit.
import { resolvedIssueFilters } from "../../../../../../features/issues/filters/rich/lazy.js";
// Deep import: the issues barrel is eager through this route, so a
// feature barrel here would pin every project-only module into the
// first-route chain (NEWFRONT-871, size gate).
import { projectsQueryOptions } from "../../../../../../features/projects/api/queries.js";
import { registerCommands } from "../../../../../../shared/commands/index.js";
import { notifyError } from "../../../../../../shared/shell/index.js";

// The sheet is a heavy leaf (picker cells, sort menus, virtualizer). Keep
// it out of the initial route chunk; the stored layout decides when to
// request it.
const LazySpreadsheetLayout = React.lazy(() =>
  import("../../../../../../features/issues/spreadsheet/SpreadsheetLayout.js").then((module) => ({
    default: module.SpreadsheetLayout,
  }))
);

// The create dialog carries the property strip and every picker. It opens
// from the header button or the shell's `ni`, so load it on first use and
// keep it out of the route's initial chain (ISS-117).
const LazyWorkItemCreateDialog = React.lazy(() =>
  import("../../../../../../features/issues/components/WorkItemCreateDialog.js").then((module) => ({
    default: module.WorkItemCreateDialog,
  }))
);

// The timeline is only rendered when the stored layout selects it. Its
// boundary module would otherwise join the route's initial closure even
// though the list first paint never mounts it (Quality gates).
const LazyGanttTimeline = React.lazy(() =>
  import("../../../../../../features/issues/gantt/GanttTimeline.js").then((module) => ({
    default: module.GanttTimeline,
  }))
);

export interface IssuesListContentProps {
  workspaceSlug: string;
  projectId: string;
  search: IssueSearch;
  apiClient: ApiClient;
  queryClient: QueryClient;
  peekKey: string | null;
  peekProjectId: string | null;
  onPeekChange: (request: PeekRequest) => void;
}

export function IssuesListContent({
  workspaceSlug,
  projectId,
  search,
  apiClient,
  queryClient,
  peekKey,
  peekProjectId,
  onPeekChange,
}: IssuesListContentProps): React.ReactElement {
  const { data: projects } = useSuspenseQuery(projectsQueryOptions(apiClient, workspaceSlug));
  const project = projects.find((entry) => entry.id === projectId) ?? null;
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
  // The entry needs a project the user may file into. Workspace standing
  // alone is not enough: an admin who is only a guest on every project
  // would otherwise see a primary button that opens the no-project notice.
  const canCreate = creatable.length > 0;

  const { data: preferences } = useSuspenseQuery(
    prefsQueryOptions(apiClient, workspaceSlug, { scope: "project", projectId: projectId })
  );
  const { data: issues } = useSuspenseQuery(
    issuesQueryOptions(apiClient, workspaceSlug, projectId, search, preferences.rich_filters, orderByOf(preferences))
  );
  // The stored rich filters (ISS-095–099) scope the list's first page and
  // every later page; without conditions this is the plain flat read. The
  // list layout owns that page (its own chunk) and reads it itself, so the
  // route only needs the flat totals for the layout root.
  React.useEffect(
    () =>
      registerCommands([
        {
          id: "issues.refresh-list",
          title: "Refresh issue list",
          hint: project?.name ?? projectId,
          section: "Issues",
          run: () => {
            void invalidateWorkspace(queryClient, workspaceSlug).catch(() => {
              notifyError("Could not refresh issues", "Check your connection and try again.");
            });
          },
        },
      ]),
    [queryClient, workspaceSlug, projectId, project?.name]
  );

  if (!project) {
    return (
      <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">Project not found</h2>
        <p className="text-body text-(--text-muted)">Pick another project from the sidebar.</p>
      </section>
    );
  }

  // Layout slices extend the offered list with their renderer; the
  // Display control resolves the same active layout from it.
  const offered: readonly LayoutKind[] = ["list", "kanban", "gantt_chart", "spreadsheet", "calendar"];
  const weekStart = useWeekStart();
  const canEdit = canEditInProject({ memberRole: project.member_role, archived: project.archived_at != null });
  const addWorkItem = canCreate ? (
    <Button
      variant="primary"
      size="small"
      onClick={() => {
        setCreateOpen(true);
      }}
    >
      Add work item
    </Button>
  ) : null;
  const display = (
    <DisplayControl
      workspaceSlug={workspaceSlug}
      entity={{ scope: "project", projectId: projectId }}
      offered={offered}
      hideCycle={!project.cycle_view}
      hideModule={!project.module_view}
    />
  );
  const analytics = (
    <ResolvedAnalyticsEntry
      workspaceSlug={workspaceSlug}
      projectId={projectId}
      projectName={project.name}
      rows={issues.results}
      totalCount={issues.total_count}
    />
  );
  // A stored expression on its own keeps the first-run card (ISS-068): the
  // project list changes card only once a condition was added through the
  // row during this visit. The layout root sends a zero total to
  // `emptyState` and a filtered page to `noMatchState`; both show the card
  // this rule picks, and the entries follow the project standing.
  const renderWithFilters = (richFilter: RichFilterBuilderParts): React.ReactElement => {
    const emptyState = (
      <ProjectIssuesEmptyState
        hasCompleteCondition={richFilter.hasCompleteCondition}
        addedThisVisit={richFilter.addedThisVisit}
        visibleCount={issues.total_count}
        canAct={canEdit}
        onClear={richFilter.clearFilters}
        onCreate={() => {
          setCreateOpen(true);
        }}
      />
    );
    return (
      <IssuesLayoutRoot
        workspaceSlug={workspaceSlug}
        entity={{ scope: "project", projectId: projectId }}
        title={project.name}
        totalCount={issues.total_count}
        resultCount={issues.results.length}
        offered={offered}
        renderLayout={(layout, prefs) => {
          const active = resolveActiveLayout(layout, offered);
          if (active === "gantt_chart") {
            return (
              <React.Suspense fallback={<ListSkeleton />}>
                <LazyGanttTimeline
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  projectIdentifier={project.identifier}
                  issues={issues.results}
                  totalCount={issues.total_count}
                  prefs={prefs}
                  peekKey={peekKey}
                  onPeekChange={onPeekChange}
                  peekProjectId={peekProjectId}
                />
              </React.Suspense>
            );
          }
          if (active === "spreadsheet") {
            return (
              <React.Suspense fallback={<ListSkeleton />}>
                <LazySpreadsheetLayout
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  projectIdentifier={project.identifier}
                  entity={{ scope: "project", projectId: projectId }}
                  prefs={prefs}
                  canEdit={canEdit}
                  hideCycle={!project.cycle_view}
                  hideModule={!project.module_view}
                  hideEstimate={project.estimate === null}
                  richExpression={preferences.rich_filters}
                  weekStart={weekStart}
                  peekKey={peekKey}
                  onPeekChange={onPeekChange}
                  peekProjectId={peekProjectId}
                />
              </React.Suspense>
            );
          }
          if (active === "calendar") {
            return (
              <React.Suspense fallback={<ListSkeleton />}>
                <LazyCalendarEntry
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  projectIdentifier={project.identifier}
                  entity={{ scope: "project", projectId: projectId }}
                  canEdit={canEdit}
                  peekKey={peekKey}
                  onPeekChange={onPeekChange}
                  peekProjectId={peekProjectId}
                />
              </React.Suspense>
            );
          }
          if (active === "kanban") {
            const ResolvedKanbanLayout = resolvedKanbanLayout();
            return (
              <ResolvedKanbanLayout
                workspaceSlug={workspaceSlug}
                entity={{ scope: "project", projectId: projectId }}
                projectId={projectId}
                projectIdentifier={project.identifier}
                prefs={prefs}
                canEdit={canEdit}
                richFilters={preferences.rich_filters}
                peekKey={peekKey}
                onPeekChange={onPeekChange}
                peekProjectId={peekProjectId}
              />
            );
          }
          const ResolvedListLayout = resolvedListLayout();
          return (
            <ResolvedListLayout
              workspaceSlug={workspaceSlug}
              entity={{ scope: "project", projectId: projectId }}
              projectId={projectId}
              projectIdentifier={project.identifier}
              prefs={prefs}
              canEdit={canEdit}
              richFilters={preferences.rich_filters}
              peekKey={peekKey}
              onPeekChange={onPeekChange}
              peekProjectId={peekProjectId}
              weekStart={weekStart}
              compact={search.layout === "compact"}
            />
          );
        }}
        emptyState={emptyState}
        noMatchState={emptyState}
        headerActions={
          <>
            {addWorkItem}
            {display}
          </>
        }
        displayControl={display}
        filterControl={richFilter.control}
        filterRow={richFilter.row}
        analyticsControl={analytics}
      />
    );
  };

  const createDialog = createOpen ? (
    <React.Suspense fallback={null}>
      <LazyWorkItemCreateDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        workspaceSlug={workspaceSlug}
        projects={creatable}
        initialProjectId={projectId}
      />
    </React.Suspense>
  ) : null;
  // The chunk is already resolved by `beforeLoad`; rendering the component
  // directly keeps the first commit the screen itself, with no Suspense
  // fallback to discard and remount (NEWFRONT-911, as NEWFRONT-906 for the
  // screen).
  const IssueFilters = resolvedIssueFilters();
  return (
    <>
      <IssueFilters
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        entity={{ scope: "project", projectId: projectId }}
        hideCycle={!project.cycle_view}
        hideModule={!project.module_view}
        viewsEnabled={project.issue_views_view}
      >
        {renderWithFilters}
      </IssueFilters>
      {createDialog}
    </>
  );
}
