// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project issues (parity/specs/issues.md §1 and §9, ISS-001–006 and
// ISS-075–081/ISS-106). Thin: the single search param validated by zod,
// a loader that prefetches display preferences and then the issue list in
// the user's stored order, and the per-entity layout root with the
// Display control in both headers. Layout slices extend the offered list
// with their renderer; the timeline is a lazy leaf so its chart code stays
// out of the route chunk; context empty-state copy arrives with ISS-068.

import { useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { Button } from "@pidash/kit";
import * as React from "react";

import { invalidateWorkspace } from "../../../../../../core/query/optimistic.js";
import { isProjectGateError } from "../../../../../../features/auth/index.js";
import {
  CalendarEntry,
  DisplayControl,
  IssuesLayoutRoot,
  ListSkeleton,
  issuesQueryOptions,
  parseIssueSearch,
  prefsQueryOptions,
  resolveActiveLayout,
  useWeekStart,
  selectCreatableProjects,
  type LayoutKind,
  type WorkItemCreateProject,
} from "../../../../../../features/issues/index.js";
import type { RichFilterBuilderParts } from "../../../../../../features/issues/filters/rich/RichFilterBuilder.js";
import {
  ResolvedAnalyticsEntry,
  loadIssueListLeaves,
  primeIssueListLeaves,
} from "../../../../../../features/issues/components/lazyLeaves.js";
import { loadListLayout, resolvedListLayout } from "../../../../../../features/issues/list/lazy.js";
import { orderByOf } from "../../../../../../features/issues/prefs/prefsQuery.js";
import { canEditInProject } from "../../../../../../shared/shell/contextual/index.js";

// The list layout is a separate chunk so the route stays inside the
// initial-JS budget (Quality gates). `beforeLoad` awaits it with the data,
// so the component mounts once instead of suspending after commit.
import { LazyIssueFilters, preloadIssueFilters } from "../../../../../../features/issues/filters/rich/lazy.js";
// Start the filter chunk with the route module so the first layout render
// does not suspend and mount the layout twice (NEWFRONT-619).
preloadIssueFilters();
import { projectsQueryOptions } from "../../../../../../features/projects/index.js";
import { registerCommands } from "../../../../../../shared/commands/index.js";
import { notifyError } from "../../../../../../shared/shell/index.js";

// Start the leaf chunk with the route module, so the warm start does not
// serialize a round trip behind the data reads; `beforeLoad` awaits it.
primeIssueListLeaves();

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

function ProjectIssuesEmptyState(): React.ReactElement {
  return (
    <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
      <h2 className="text-h3 text-(--text)">No issues yet</h2>
      <p className="text-body text-(--text-muted)">New issues in this project will appear here.</p>
    </section>
  );
}

function ProjectIssuesNoMatchState(): React.ReactElement {
  return (
    <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
      <h2 className="text-h3 text-(--text)">No matching issues</h2>
      <p className="text-body text-(--text-muted)">No issues match the current filters.</p>
    </section>
  );
}

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/issues/")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  beforeLoad: async ({ context, params, search }) => {
    try {
      // Preferences first: the stored order is part of the list query, so
      // the timeline and the list both ask for the order the user chose.
      const record = await context.queryClient.ensureQueryData(
        prefsQueryOptions(context.apiClient, params.ws, { scope: "project", projectId: params.projectId })
      );
      await context.queryClient.ensureQueryData(
        issuesQueryOptions(
          context.apiClient,
          params.ws,
          params.projectId,
          search,
          record.rich_filters,
          orderByOf(record)
        )
      );
      // The list and analytics leaves, and the list layout itself, ride
      // their own chunks (Quality gates); await them with the data so the
      // first commit is the screen itself. The list layout reads its own
      // first page, including the stored sub-issue and empty-group flags.
      await Promise.all([loadIssueListLeaves(), loadListLayout()]);
    } catch (error) {
      // Denial statuses belong to the project gate (AUTH-024): let them
      // through so the gate renders the denial screen instead of the load
      // error.
      if (!isProjectGateError(error)) throw error;
    }
  },
  loader: async ({ context, params }) => {
    await context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws));
  },
  // The stored layout is unknown while the prefetch runs (it is fetched,
  // not in the URL), so the pending window always shows the list
  // placeholder; the layout renders its own skeleton once mounted.
  pendingComponent: function IssuesPending() {
    return <ListSkeleton />;
  },
  errorComponent: function IssueListError({ error, reset }: { error: unknown; reset: () => void }) {
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
  component: function IssueListScreen() {
    const params = Route.useParams();
    const search = Route.useSearch();
    const context = Route.useRouteContext();
    const queryClient = context.queryClient;
    const navigate = useNavigate();
    const peekKey = search.peekIssueId ?? null;
    const setPeekKey = (key: string | null): void => {
      const next = { ...search };
      if (key === null) delete next.peekIssueId;
      else next.peekIssueId = key;
      void navigate({ to: ".", search: next, replace: true } as never);
    };

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
    // The entry needs a project the user may file into. Workspace standing
    // alone is not enough: an admin who is only a guest on every project
    // would otherwise see a primary button that opens the no-project notice.
    const canCreate = creatable.length > 0;

    const { data: preferences } = useSuspenseQuery(
      prefsQueryOptions(context.apiClient, params.ws, { scope: "project", projectId: params.projectId })
    );
    const { data: issues } = useSuspenseQuery(
      issuesQueryOptions(
        context.apiClient,
        params.ws,
        params.projectId,
        search,
        preferences.rich_filters,
        orderByOf(preferences)
      )
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
            hint: project?.name ?? params.projectId,
            section: "Issues",
            run: () => {
              void invalidateWorkspace(queryClient, params.ws).catch(() => {
                notifyError("Could not refresh issues", "Check your connection and try again.");
              });
            },
          },
        ]),
      [queryClient, params.ws, params.projectId, project?.name]
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
    const offered: readonly LayoutKind[] = ["list", "gantt_chart", "spreadsheet", "calendar"];
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
        workspaceSlug={params.ws}
        entity={{ scope: "project", projectId: params.projectId }}
        offered={offered}
        hideCycle={!project.cycle_view}
        hideModule={!project.module_view}
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
    const renderWithFilters = (richFilter: RichFilterBuilderParts | null): React.ReactElement => (
      <IssuesLayoutRoot
        workspaceSlug={params.ws}
        entity={{ scope: "project", projectId: params.projectId }}
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
                  workspaceSlug={params.ws}
                  projectId={params.projectId}
                  projectIdentifier={project.identifier}
                  issues={issues.results}
                  totalCount={issues.total_count}
                  prefs={prefs}
                />
              </React.Suspense>
            );
          }
          if (active === "spreadsheet") {
            return (
              <React.Suspense fallback={<ListSkeleton />}>
                <LazySpreadsheetLayout
                  workspaceSlug={params.ws}
                  projectId={params.projectId}
                  projectIdentifier={project.identifier}
                  entity={{ scope: "project", projectId: params.projectId }}
                  prefs={prefs}
                  canEdit={canEdit}
                  hideCycle={!project.cycle_view}
                  hideModule={!project.module_view}
                  hideEstimate={project.estimate === null}
                  richExpression={preferences.rich_filters}
                  weekStart={weekStart}
                />
              </React.Suspense>
            );
          }
          if (active === "calendar") {
            return (
              <CalendarEntry
                workspaceSlug={params.ws}
                projectId={params.projectId}
                projectIdentifier={project.identifier}
                entity={{ scope: "project", projectId: params.projectId }}
                canEdit={canEdit}
              />
            );
          }
          const ResolvedListLayout = resolvedListLayout();
          return (
            <ResolvedListLayout
              workspaceSlug={params.ws}
              entity={{ scope: "project", projectId: params.projectId }}
              projectId={params.projectId}
              projectIdentifier={project.identifier}
              prefs={prefs}
              canEdit={canEdit}
              richFilters={preferences.rich_filters}
              peekKey={peekKey}
              onPeekChange={setPeekKey}
              compact={search.layout === "compact"}
            />
          );
        }}
        emptyState={<ProjectIssuesEmptyState />}
        noMatchState={<ProjectIssuesNoMatchState />}
        headerActions={
          <>
            {addWorkItem}
            {display}
          </>
        }
        displayControl={display}
        filterControl={richFilter?.control}
        filterRow={richFilter?.row}
        analyticsControl={analytics}
      />
    );
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
            hideCycle={!project.cycle_view}
            hideModule={!project.module_view}
            viewsEnabled={project.issue_views_view}
          >
            {renderWithFilters}
          </LazyIssueFilters>
        </React.Suspense>
        {createDialog}
      </>
    );
  },
});
