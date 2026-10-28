// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle detail body (parity/specs/cycles.md §13, CYC-032–034; §14,
// CYC-035). Split from the eager route module (size gate): the route keeps
// its loader, while the page composition and its issue-layout dependencies
// load with this chunk.

import { useSuspenseQuery } from "@tanstack/react-query";
import { Badge, Button } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../../../../core/api/client.js";
import { useProjectRoles, useProjectRow } from "../../../../../../features/auth/queries.js";
import { cycleDetailQueryOptions } from "../../../../../../features/cycles/api/cycleDetail.js";
import { useCycleSidebarCollapsed } from "../../../../../../features/cycles/api/cycleSidebarState.js";
import { CycleDetailCrumbs } from "../../../../../../features/cycles/components/CycleDetailCrumbs.js";
import { CycleDetailSidebar } from "../../../../../../features/cycles/components/CycleDetailSidebar.js";
import { CycleRowMenu } from "../../../../../../features/cycles/components/CycleRowMenu.js";
import { CycleMemberDates } from "../../../../../../features/cycles/components/CycleMemberDates.js";
import { CyclePaletteEntries } from "../../../../../../features/cycles/components/CyclePaletteEntries.js";
import { CycleSidebarToggle } from "../../../../../../features/cycles/components/CycleSidebarToggle.js";
import { CYCLE_TRANSFER_TITLE } from "../../../../../../features/cycles/components/CycleTransferDialog.js";
import { CycleTransferAction } from "../../../../../../features/cycles/components/CycleTransferAction.js";
import { FinishedCycleNotice } from "../../../../../../features/cycles/components/FinishedCycleNotice.js";
import { canTransferFromBanner } from "../../../../../../features/cycles/lib/cycleTransfer.js";
import { isProjectAuthorized } from "../../../../../../features/projects/tabs.js";
import { projectsQueryOptions } from "../../../../../../features/projects/api/queries.js";
import { isCompletedCycle } from "../../../../../../shared/pickers/cycles.js";
import { canEditInProject } from "../../../../../../shared/shell/contextual/index.js";
import { DisplayControl } from "../../../../../../features/issues/display/DisplayControl.js";
import { LAYOUT_ORDER, resolveActiveLayout, type LayoutKind } from "../../../../../../features/issues/layouts/kinds.js";
import { IssuesLayoutRoot } from "../../../../../../features/issues/layouts/IssuesLayoutRoot.js";
import { ListSkeleton } from "../../../../../../features/issues/loaders/ListSkeleton.js";
import { selectDisplayPrefs } from "../../../../../../features/issues/prefs/displayPrefs.js";
import { prefsQueryOptions } from "../../../../../../features/issues/prefs/prefsQuery.js";
import { listPageQueryOptions } from "../../../../../../features/issues/list/pages.js";
import { ResolvedAnalyticsEntry } from "../../../../../../features/issues/components/lazyLeaves.js";
import { resolvedListLayout } from "../../../../../../features/issues/list/lazy.js";
import { resolvedKanbanLayout } from "../../../../../../features/issues/kanban/lazy.js";
import { CycleIssuesEmpty, FilteredEmptyCard } from "../../../../../../features/issues/emptyStates/cards.js";
import { isFilteredEmpty } from "../../../../../../features/issues/emptyStates/filteredEmpty.js";
import { isConditionComplete, parseRichExpression } from "../../../../../../features/issues/filters/rich/expression.js";
import type { RichFilterBuilderParts } from "../../../../../../features/issues/filters/rich/RichFilterBuilder.js";
import { LazyIssueFilters, preloadIssueFilters } from "../../../../../../features/issues/filters/rich/lazy.js";
import { useAttachExisting } from "../../../../../../features/issues/list/attach.js";
import { selectCreatableProjects } from "../../../../../../features/issues/create/createProjects.js";
import type { WorkItemCreateProject } from "../../../../../../features/issues/components/WorkItemCreateDialog.js";
import { useWeekStart } from "../../../../../../features/issues/dropdowns/weekStart.js";
import type { PeekRequest } from "../../../../../../features/issues/peek/peekUrl.js";

preloadIssueFilters();

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
const LazyWorkItemCreateDialog = React.lazy(() =>
  import("../../../../../../features/issues/components/WorkItemCreateDialog.js").then((module) => ({
    default: module.WorkItemCreateDialog,
  }))
);
// The empty card's picker rides its own chunk, as it does inside the list
// layout (Quality gates, initial-JS budget).
const LazyAddExistingModal = React.lazy(() =>
  import("../../../../../../features/issues/list/AddExistingModal.js").then((module) => ({
    default: module.AddExistingModal,
  }))
);

const NOOP = (): void => {};

export interface CycleDetailContentProps {
  workspaceSlug: string;
  projectId: string;
  cycleId: string;
  peekKey: string | null;
  peekProjectId: string | null;
  onPeekChange: (request: PeekRequest) => void;
  compact: boolean;
}

/**
 * Detail body behind the gone-away gate: the gate owns the missing
 * cycle, so every read below suspends on a live one.
 */
export function CycleDetailContent({
  workspaceSlug,
  projectId,
  cycleId,
  peekKey,
  peekProjectId,
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
  // The estimate link comes from the retrieve row: the projects list
  // answers a fixed .values() projection without it (EST-019).
  const estimatesEnabled = projectRow.data?.estimate != null;
  const [sidebarCollapsed, setSidebarCollapsed] = useCycleSidebarCollapsed();
  // The stored rich filters scope this read, as on the project list:
  // the totals, the badge, the analytics rows and the empty-card decision
  // all follow the filtered list, so a match-nothing expression routes to
  // the filtered card instead of an empty layout (ISS-069).
  const { data: preferences } = useSuspenseQuery(prefsQueryOptions(client, workspaceSlug, entity));
  const richFilters = preferences.rich_filters;
  const showSubIssues = selectDisplayPrefs(preferences).showSubIssues;
  const { data: firstPage } = useSuspenseQuery(
    listPageQueryOptions(client, workspaceSlug, {
      entity,
      groupBy: null,
      orderBy: "sort_order",
      showSubIssues,
      richFilters,
    })
  );
  const [createOpen, setCreateOpen] = React.useState(false);
  const [addExistingOpen, setAddExistingOpen] = React.useState(false);
  const weekStart = useWeekStart();
  // The host owns both empty-card entries: an empty page never mounts a
  // layout, so no layout ever registers its overlays here (ISS-069).
  const { attach } = useAttachExisting({ workspaceSlug, projectId, entity, cycleId });
  async function attachExisting(ids: string[]): Promise<void> {
    if (await attach(ids)) setAddExistingOpen(false);
  }

  if (project === null) {
    return (
      <section aria-label="Cycle details" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">Project not found</h2>
        <p className="text-body text-(--text-muted)">Pick another project from the sidebar.</p>
      </section>
    );
  }

  const finished = isCompletedCycle(cycle.status);
  const canMutate = canEditInProject({
    memberRole: project.member_role,
    archived: project.archived_at !== null,
  });
  const canEdit = canMutate && !finished;
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
          setCreateOpen(true);
        }}
        onAddExisting={() => {
          setAddExistingOpen(true);
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
                  entity={entity}
                  completedCycle={finished}
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
                  entity={entity}
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
                  entity={entity}
                  canEdit={canEdit}
                  canAddExisting={canCreate}
                  membership={{ cycleId }}
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
                peekProjectId={peekProjectId}
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
              peekProjectId={peekProjectId}
              compact={compact}
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
            {canMutate && cycle.archived_at == null ? (
              <CycleRowMenu
                workspaceSlug={workspaceSlug}
                projectId={projectId}
                cycle={cycle}
                canMutate={canMutate}
                currentProject={{ id: projectId, name: project.name, identifier: project.identifier }}
                alwaysVisible
              />
            ) : null}
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
  // Same picker the list layout mounts for a cycle scope; the host owns it
  // so the empty card reaches it with no layout mounted (ISS-069).
  const addExistingDialog = addExistingOpen ? (
    <React.Suspense fallback={null}>
      <LazyAddExistingModal
        open
        onOpenChange={(open) => {
          if (!open) setAddExistingOpen(false);
        }}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        excludeCycleMembers
        onAttach={(ids) => attachExisting(ids)}
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
            estimatesEnabled={estimatesEnabled}
            editable={canEdit}
            onClose={() => setSidebarCollapsed(true)}
          />
        )}
      </div>
      {createDialog}
      {addExistingDialog}
    </>
  );
}
