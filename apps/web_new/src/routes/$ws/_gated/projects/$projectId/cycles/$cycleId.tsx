// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle detail (parity/specs/cycles.md §13, CYC-032–034; §14, CYC-035).
// The Cycles area owns the full page: the breadcrumb trail with the
// cycle switcher, the stored dates in the member timezone (PROF-017,
// NEWFRONT-859), and the shared issues layouts bound to the
// { scope: "cycle" } entity with per-cycle preferences beside the
// collapsible sidebar. The header actions are the work-item count
// badge, the member+ add-item button (absent on finished cycles), the
// display control and the sidebar toggle; analytics rides the shared
// entry with the cycle scope. A missing, deleted or archived cycle
// renders the area's gone-away view through the detail gate (CYC-013).
// The page registers the cycle's palette entries (CYC-030) and carries
// the finished read-only notice with the transfer action (CYC-031) on
// completed cycles.

import { useSuspenseQuery } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { isApiError } from "@pidash/api-client";
import { Badge, Button } from "@pidash/kit";
import * as React from "react";

// Direct module imports: this route definition is eager, and the
// feature barrels would pull the list body, dialogs and picker parts
// into the initial bundle (NEWFRONT-619, size gate).
import { CycleDetailCrumbs } from "../../../../../../features/cycles/components/CycleDetailCrumbs.js";
import { CycleDetailGate } from "../../../../../../features/cycles/components/CycleDetailGate.js";
import { CycleDetailSidebar } from "../../../../../../features/cycles/components/CycleDetailSidebar.js";
import { CycleMemberDates } from "../../../../../../features/cycles/components/CycleMemberDates.js";
import { CycleSidebarToggle } from "../../../../../../features/cycles/components/CycleSidebarToggle.js";
import { CyclePaletteEntries } from "../../../../../../features/cycles/components/CyclePaletteEntries.js";
import { CYCLE_TRANSFER_TITLE } from "../../../../../../features/cycles/components/CycleTransferDialog.js";
import { CycleTransferAction } from "../../../../../../features/cycles/components/CycleTransferAction.js";
import { FinishedCycleNotice } from "../../../../../../features/cycles/components/FinishedCycleNotice.js";
import { canTransferFromBanner } from "../../../../../../features/cycles/lib/cycleTransfer.js";
import { cycleDetailQueryOptions } from "../../../../../../features/cycles/api/cycleDetail.js";
import { useCycleSidebarCollapsed } from "../../../../../../features/cycles/api/cycleSidebarState.js";
import { getAppClient } from "../../../../../../core/api/client.js";
import { useProjectRoles, useProjectRow } from "../../../../../../features/auth/queries.js";
import { isProjectAuthorized, projectsQueryOptions } from "../../../../../../features/projects/index.js";
import { isCompletedCycle } from "../../../../../../shared/pickers/cycles.js";
import { canEditInProject } from "../../../../../../shared/shell/contextual/index.js";
import { DisplayControl } from "../../../../../../features/issues/display/DisplayControl.js";
import { LAYOUT_ORDER, resolveActiveLayout, type LayoutKind } from "../../../../../../features/issues/layouts/kinds.js";
import { IssuesLayoutRoot } from "../../../../../../features/issues/layouts/IssuesLayoutRoot.js";
import { ListSkeleton } from "../../../../../../features/issues/loaders/ListSkeleton.js";
import { selectDisplayPrefs } from "../../../../../../features/issues/prefs/displayPrefs.js";
import { prefsQueryOptions } from "../../../../../../features/issues/prefs/prefsQuery.js";
import { listPageQueryOptions } from "../../../../../../features/issues/list/pages.js";
import {
  ResolvedAnalyticsEntry,
  loadIssueListLeaves,
  primeIssueListLeaves,
} from "../../../../../../features/issues/components/lazyLeaves.js";
import { resolvedListLayout } from "../../../../../../features/issues/list/lazy.js";
import { loadKanbanLayout, resolvedKanbanLayout } from "../../../../../../features/issues/kanban/lazy.js";
import { CycleIssuesEmpty, FilteredEmptyCard } from "../../../../../../features/issues/emptyStates/cards.js";
import { isFilteredEmpty } from "../../../../../../features/issues/emptyStates/filteredEmpty.js";
import { isConditionComplete, parseRichExpression } from "../../../../../../features/issues/filters/rich/expression.js";
import type { RichFilterBuilderParts } from "../../../../../../features/issues/filters/rich/RichFilterBuilder.js";
import { LazyIssueFilters, preloadIssueFilters } from "../../../../../../features/issues/filters/rich/lazy.js";
import { parseIssueSearch } from "../../../../../../features/issues/filters/search.js";
import type { ListLayoutOverlays } from "../../../../../../features/issues/list/ListLayout.js";
import { selectCreatableProjects } from "../../../../../../features/issues/create/createProjects.js";
import type { WorkItemCreateProject } from "../../../../../../features/issues/components/WorkItemCreateDialog.js";
import { useWeekStart } from "../../../../../../features/issues/dropdowns/weekStart.js";

// Start the leaf and filter chunks with the route module so the first
// layout render does not suspend and mount twice (NEWFRONT-619).
primeIssueListLeaves();
preloadIssueFilters();

// The calendar, sheet and timeline ride their own chunks; the stored
// layout decides when to request them (Quality gates).
const LazyCalendarEntry = React.lazy(() =>
  import("../../../../../../features/issues/calendar/CalendarEntry.js").then((module) => ({
    default: module.CalendarEntry,
  }))
);
const LazySpreadsheetLayout = React.lazy(() =>
  import("../../../../../../features/issues/spreadsheet/SpreadsheetLayout.js").then((module) => ({
    default: module.SpreadsheetLayout,
  }))
);
const LazyGanttTimeline = React.lazy(() =>
  import("../../../../../../features/issues/gantt/GanttTimeline.js").then((module) => ({
    default: module.GanttTimeline,
  }))
);

// The create dialog carries the property strip and every picker; it
// opens from the header button, so load it on first use (ISS-117).
const LazyWorkItemCreateDialog = React.lazy(() =>
  import("../../../../../../features/issues/components/WorkItemCreateDialog.js").then((module) => ({
    default: module.WorkItemCreateDialog,
  }))
);

/** The clear entry stays inert while the filter row is still loading. */
const NOOP = (): void => {};

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/cycles/$cycleId")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  beforeLoad: async ({ context, params }) => {
    const { prefetchEntityList } = await import("../../../../../../features/issues/list/prefetch.js");
    const entity = { scope: "cycle", projectId: params.projectId, cycleId: params.cycleId } as const;
    try {
      // Preferences first: the stored flags shape the first page read.
      await prefetchEntityList(context, params.ws, entity);
      await context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws));
      await context.queryClient.ensureQueryData(
        cycleDetailQueryOptions(context.apiClient, params.ws, params.projectId, params.cycleId)
      );
      // The board and the list/analytics leaves ride their own chunks;
      // await them with the data so the first commit is the screen.
      await Promise.all([loadKanbanLayout(), loadIssueListLeaves()]);
    } catch (error) {
      // A missing cycle 404s the prefetch: let the gone-away gate own it
      // instead of failing the load (CYC-013).
      if (!isApiError(error) || error.status !== 404) throw error;
    }
  },
  pendingComponent: function CycleIssuesPending() {
    return <ListSkeleton />;
  },
  errorComponent: function CycleDetailError({ error, reset }: { error: unknown; reset: () => void }) {
    return (
      <section aria-label="Cycle details" className="flex flex-col items-center gap-(--space-4) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">Could not load cycle details</h2>
        <p className="text-body text-(--text-muted)">
          {error instanceof Error ? error.message : "Something went wrong."}
        </p>
        <Button variant="primary" onClick={reset}>
          Try again
        </Button>
      </section>
    );
  },
  component: function CycleDetailRoute() {
    const params = Route.useParams();
    const search = Route.useSearch();
    const navigate = useNavigate();
    const setPeekKey = (key: string | null): void => {
      const next = { ...search };
      if (key === null) delete next.peekIssueId;
      else next.peekIssueId = key;
      void navigate({ to: ".", search: next, replace: true } as never);
    };
    return (
      <CycleDetailGate workspaceSlug={params.ws} projectId={params.projectId} cycleId={params.cycleId}>
        <CycleDetailContent
          workspaceSlug={params.ws}
          projectId={params.projectId}
          cycleId={params.cycleId}
          peekKey={search.peekIssueId ?? null}
          onPeekChange={setPeekKey}
          compact={search.layout === "compact"}
        />
      </CycleDetailGate>
    );
  },
});

interface CycleDetailContentProps {
  workspaceSlug: string;
  projectId: string;
  cycleId: string;
  peekKey: string | null;
  onPeekChange: (key: string | null) => void;
  compact: boolean;
}

/**
 * Detail body behind the gone-away gate: the gate owns the missing
 * cycle, so every read below suspends on a live one.
 */
function CycleDetailContent({
  workspaceSlug,
  projectId,
  cycleId,
  peekKey,
  onPeekChange,
  compact,
}: CycleDetailContentProps): React.ReactElement {
  const client = getAppClient();
  const entity = { scope: "cycle", projectId, cycleId } as const;
  const { data: projects } = useSuspenseQuery(projectsQueryOptions(client, workspaceSlug));
  const project = projects.find((entry) => entry.id === projectId) ?? null;
  const { data: cycle } = useSuspenseQuery(cycleDetailQueryOptions(client, workspaceSlug, projectId, cycleId));
  const roles = useProjectRoles(workspaceSlug);
  const projectRow = useProjectRow(workspaceSlug, projectId);
  const projectTimeZone = projectRow.data?.timezone ?? "UTC";
  const [sidebarCollapsed, setSidebarCollapsed] = useCycleSidebarCollapsed();
  // Stored rich filters scope the layouts; the first page only feeds
  // the totals and the empty-card decision.
  const { data: preferences } = useSuspenseQuery(prefsQueryOptions(client, workspaceSlug, entity));
  const richFilters = preferences.rich_filters;
  const showSubIssues = selectDisplayPrefs(preferences).showSubIssues;
  const { data: firstPage } = useSuspenseQuery(
    listPageQueryOptions(client, workspaceSlug, {
      entity,
      groupBy: null,
      orderBy: "sort_order",
      showSubIssues,
    })
  );
  const [overlays, setOverlays] = React.useState<ListLayoutOverlays | null>(null);
  const [createOpen, setCreateOpen] = React.useState(false);
  const weekStart = useWeekStart();

  if (project === null) {
    return (
      <section aria-label="Cycle details" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">Project not found</h2>
        <p className="text-body text-(--text-muted)">Pick another project from the sidebar.</p>
      </section>
    );
  }

  const finished = isCompletedCycle(cycle.status);
  const canEdit =
    canEditInProject({ memberRole: project.member_role, archived: project.archived_at !== null }) && !finished;
  const canCreate = isProjectAuthorized(roles.data?.[projectId] ?? null) && !finished;
  const creatable: WorkItemCreateProject[] = selectCreatableProjects(
    projects.map((entry) => ({
      id: entry.id,
      name: entry.name,
      identifier: entry.identifier,
      memberRole: entry.member_role,
      archived: entry.archived_at !== null,
    }))
  );
  // Layout slices extend the offered list with their renderer; the
  // switcher offers them in canonical order.
  const offered: readonly LayoutKind[] = LAYOUT_ORDER;
  const flatResults = Array.isArray(firstPage.results) ? firstPage.results : [];
  const resultCount = Array.isArray(firstPage.results) ? firstPage.results.length : firstPage.count;
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
  const badge =
    firstPage.total_count > 0 ? (
      <span data-testid="cycle-detail-badge">
        <Badge>{firstPage.total_count}</Badge>
      </span>
    ) : null;
  const display = (
    <DisplayControl
      workspaceSlug={workspaceSlug}
      entity={entity}
      offered={offered}
      hideCycle={!project.cycle_view}
      hideModule={!project.module_view}
    />
  );
  const flipSidebar = (): void => {
    setSidebarCollapsed(!sidebarCollapsed);
  };
  const analytics = (
    <ResolvedAnalyticsEntry
      workspaceSlug={workspaceSlug}
      projectId={projectId}
      projectName={project.name}
      rows={flatResults}
      totalCount={firstPage.total_count}
      cycle={{ id: cycleId, name: cycle.name }}
      narrowGuestAccess
    />
  );
  // The cycle keys the card on its stored expression as soon as it
  // loads; a finished cycle reads as a record before any filter is
  // considered (ISS-069/070).
  const filtered =
    !finished &&
    isFilteredEmpty({
      mode: "stored",
      hasCompleteCondition: parseRichExpression(richFilters).some(isConditionComplete),
      addedThisVisit: false,
      visibleCount: firstPage.total_count,
    });
  const renderWithFilters = (richFilter: RichFilterBuilderParts | null): React.ReactElement => {
    const filteredNow =
      filtered ||
      (!finished &&
        richFilter !== null &&
        isFilteredEmpty({
          mode: "visit",
          hasCompleteCondition: richFilter.hasCompleteCondition,
          addedThisVisit: richFilter.addedThisVisit,
          visibleCount: firstPage.total_count,
        }));
    // The clear entry stays inert while the filter row is still
    // loading; the visit-scoped rule applies once a condition is added
    // through the row, so the card follows the same predicate as the
    // project list.
    const card = filteredNow ? (
      <FilteredEmptyCard
        context="cycle"
        clearLabel="Clear filters"
        canAct={canEdit}
        onClear={richFilter === null ? NOOP : richFilter.clearFilters}
      />
    ) : (
      <CycleIssuesEmpty
        completed={finished}
        canAct={canEdit}
        onCreate={() => {
          overlays?.openCreate();
        }}
        onAddExisting={() => {
          overlays?.openAddExisting();
        }}
      />
    );
    return (
      <IssuesLayoutRoot
        workspaceSlug={workspaceSlug}
        entity={entity}
        title={cycle.name}
        totalCount={firstPage.total_count}
        resultCount={resultCount}
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
                  issues={flatResults}
                  totalCount={firstPage.total_count}
                  prefs={prefs}
                  completedCycle={finished}
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
                  entity={entity}
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
              <React.Suspense fallback={<ListSkeleton />}>
                <LazyCalendarEntry
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  projectIdentifier={project.identifier}
                  entity={entity}
                  canEdit={canEdit}
                  canAddExisting
                  membership={{ cycleId }}
                />
              </React.Suspense>
            );
          }
          if (active === "kanban") {
            const ResolvedKanbanLayout = resolvedKanbanLayout();
            return (
              <ResolvedKanbanLayout
                workspaceSlug={workspaceSlug}
                entity={entity}
                projectId={projectId}
                projectIdentifier={project.identifier}
                prefs={prefs}
                canEdit={canEdit}
                completedCycle={finished}
                richFilters={richFilters}
                cycleId={cycleId}
                peekKey={peekKey}
                onPeekChange={onPeekChange}
              />
            );
          }
          const ResolvedListLayout = resolvedListLayout();
          return (
            <ResolvedListLayout
              workspaceSlug={workspaceSlug}
              entity={entity}
              projectId={projectId}
              projectIdentifier={project.identifier}
              prefs={prefs}
              canEdit={canEdit}
              richFilters={richFilters}
              cycleId={cycleId}
              completedCycle={finished}
              peekKey={peekKey}
              onPeekChange={onPeekChange}
              compact={compact}
              onRegisterOverlays={setOverlays}
            />
          );
        }}
        emptyState={card}
        noMatchState={card}
        headerActions={
          <>
            {badge}
            {addWorkItem}
            {display}
            <CycleSidebarToggle collapsed={sidebarCollapsed} onToggle={flipSidebar} />
          </>
        }
        displayControl={display}
        filterControl={richFilter?.control}
        filterRow={<>{richFilter?.row}</>}
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
        projectIdentifier={project.identifier}
        context={{ cycleId }}
      />
    </React.Suspense>
  ) : null;
  const canTransferCycleWork = isProjectAuthorized(roles.data?.[projectId] ?? null) && canTransferFromBanner(cycle);
  return (
    <>
      <CyclePaletteEntries
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        cycle={cycle}
        memberRole={project.member_role}
        projectArchived={project.archived_at !== null}
      />
      <CycleDetailCrumbs
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectName={project.name}
        cycleId={cycleId}
        cycleName={cycle.name}
        cycleStatus={cycle.status}
        narrowActions={
          <CycleSidebarToggle
            collapsed={sidebarCollapsed}
            onToggle={flipSidebar}
            testId="cycle-sidebar-toggle-narrow"
          />
        }
      />
      <CycleMemberDates workspaceSlug={workspaceSlug} projectId={projectId} cycleId={cycleId} />
      {finished ? (
        <FinishedCycleNotice>
          {canTransferCycleWork ? (
            <CycleTransferAction
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              sourceCycleId={cycle.id}
              label={CYCLE_TRANSFER_TITLE}
            />
          ) : null}
        </FinishedCycleNotice>
      ) : null}
      <div className="flex min-h-0 flex-1 items-start gap-(--space-4)">
        <div className="min-w-0 flex-1">
          <React.Suspense fallback={renderWithFilters(null)}>
            <LazyIssueFilters
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              entity={entity}
              hideCycle={!project.cycle_view}
              hideModule={!project.module_view}
              viewsEnabled={project.issue_views_view}
            >
              {renderWithFilters}
            </LazyIssueFilters>
          </React.Suspense>
        </div>
        {sidebarCollapsed ? null : (
          <CycleDetailSidebar
            key={cycleId}
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            cycle={cycle}
            projectTimeZone={projectTimeZone}
            estimatesEnabled={project.estimate != null}
            editable={canEdit}
            onClose={() => setSidebarCollapsed(true)}
          />
        )}
      </div>
      {createDialog}
    </>
  );
}
