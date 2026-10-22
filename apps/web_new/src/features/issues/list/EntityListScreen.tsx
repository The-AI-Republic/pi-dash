// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// List layout host for cycle and module pages (parity/specs/issues.md §2,
// ISS-007–014). The cycles and modules areas own those pages; the issues
// slice only mounts the same layout root and list renderer the project
// route uses, so grouping, creation and the inline strip behave the same
// in every issues-bearing context.

import { useQuery, useSuspenseQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { canEditInProject } from "../../../shared/shell/contextual/index.js";
import { CalendarEntry } from "../calendar/CalendarEntry.js";
import { DisplayControl } from "../display/DisplayControl.js";
import type { LayoutKind } from "../layouts/kinds.js";
import { IssuesLayoutRoot } from "../layouts/IssuesLayoutRoot.js";
import { prefsQueryOptions } from "../prefs/prefsQuery.js";
import { selectDisplayPrefs } from "../prefs/displayPrefs.js";
import { listPageQueryOptions } from "./pages.js";
import { resolvedListLayout } from "./lazy.js";
import type { ListLayoutOverlays } from "./ListLayout.js";
import type { IssuesEntity } from "../prefs/entities.js";
import { pickerCyclesQueryOptions, pickerModulesQueryOptions } from "../../../shared/pickers/reference.js";
import { isCompletedCycle } from "../../../shared/pickers/cycles.js";
import { isCompletedModule } from "../../../shared/pickers/modules.js";
import { useCycleProgress } from "../transfer/api.js";
import { canTransferCycleIssues, showsCompletedCycleNote } from "../transfer/rules.js";
import { TransferIssuesDialog } from "../transfer/TransferIssuesDialog.js";
import { TransferIssuesPrompt } from "../transfer/TransferIssuesPrompt.js";
import { CycleIssuesEmpty, FilteredEmptyCard, ModuleIssuesEmpty } from "../emptyStates/cards.js";
import { isFilteredEmpty } from "../emptyStates/filteredEmpty.js";
import { isConditionComplete, parseRichExpression } from "../filters/rich/expression.js";
import type { RichFilterBuilderParts } from "../filters/rich/RichFilterBuilder.js";
import { LazyIssueFilters } from "../filters/rich/lazy.js";

export interface EntityListScreenProps {
  workspaceSlug: string;
  projectId: string;
  entity: IssuesEntity;
  /** Page title when the host already knows it; cycle/module resolve it here. */
  title?: string | undefined;
  cycleId?: string | undefined;
  moduleId?: string | undefined;
  /** Completed cycles render read-only. */
  completedCycle?: boolean | undefined;
  /** Completed modules render read-only. */
  completedModule?: boolean | undefined;
  peekKey: string | null;
  onPeekChange: (key: string | null) => void;
  compact: boolean;
}

/** Same layout root and list renderer as the project issue screen. */
export function EntityListScreen({
  workspaceSlug,
  projectId,
  entity,
  title,
  cycleId,
  moduleId,
  completedCycle,
  completedModule,
  peekKey,
  onPeekChange,
  compact,
}: EntityListScreenProps): React.ReactElement {
  const client = getAppClient();
  const { data: projects } = useSuspenseQuery(projectsQueryOptions(client, workspaceSlug));
  const project = projects.find((row) => row.id === projectId) ?? null;
  // Titles live with the layout's own chunk instead of the route: the
  // route graph is eager and the picker reference barst pulls cycle,
  // module, pod and estimate contracts into the initial bundle (NEWFRONT-619).
  const { data: cycles } = useQuery({
    ...pickerCyclesQueryOptions(client, workspaceSlug, projectId),
    enabled: entity.scope === "cycle",
  });
  const { data: modules } = useQuery({
    ...pickerModulesQueryOptions(client, workspaceSlug, projectId),
    enabled: entity.scope === "module",
  });
  const cycle = entity.scope === "cycle" ? cycles?.find((row) => row.id === entity.cycleId) : undefined;
  const loadedModule = entity.scope === "module" ? modules?.find((row) => row.id === entity.moduleId) : undefined;
  const resolvedTitle = title ?? cycle?.name ?? loadedModule?.name ?? (entity.scope === "module" ? "Module" : "Cycle");
  const resolvedCompletedCycle = completedCycle ?? isCompletedCycle(cycle?.status);
  const resolvedCompletedModule = completedModule ?? isCompletedModule(loadedModule?.status);
  // The transfer prompt pairs the picker row's completed status with the
  // progress endpoint's unfinished counts (ISS-224): the cycle list has no
  // per-group breakdown. The progress endpoint keeps serving the frozen
  // pre-move counts after a transfer, so the action stays up. The read
  // stays off for module and project scopes.
  const cycleIdForTransfer = entity.scope === "cycle" ? entity.cycleId : "";
  const progress = useCycleProgress(workspaceSlug, projectId, cycleIdForTransfer, cycleIdForTransfer !== "");
  const transferCycle = cycle === undefined ? null : { status: cycle.status, ...(progress.data ?? {}) };
  const completedNote = showsCompletedCycleNote(transferCycle);
  const canTransfer = canTransferCycleIssues(transferCycle);
  const [transferOpen, setTransferOpen] = React.useState(false);
  // The add-existing entry lives in the list layout's overlays; the empty
  // card below the list asks for it once that layout has mounted.
  const [overlays, setOverlays] = React.useState<ListLayoutOverlays | null>(null);
  // Stored rich filters scope the list in cycle/module context too.
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

  if (project === null) {
    return (
      <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">Project not found</h2>
        <p className="text-body text-(--text-muted)">Pick another project from the sidebar.</p>
      </section>
    );
  }

  // The cycle/module routes await the chunk in `beforeLoad`; render the
  // resolved component directly so the layout mounts once (NEWFRONT-619).
  const ListLayout = resolvedListLayout();
  const canEdit =
    canEditInProject({
      memberRole: project.member_role,
      archived: project.archived_at !== null,
    }) &&
    !resolvedCompletedCycle &&
    !resolvedCompletedModule;
  const offered: readonly LayoutKind[] = ["list", "calendar"];
  const display = (
    <DisplayControl
      workspaceSlug={workspaceSlug}
      entity={entity}
      offered={offered}
      hideCycle={!project.cycle_view}
      hideModule={!project.module_view}
    />
  );
  const resultCount = Array.isArray(firstPage.results) ? firstPage.results.length : firstPage.count;
  // ISS-069/070: the cycle keys the card on its stored expression as soon
  // as it loads; the module keeps the first-run card until a condition was
  // added through the row (§8). A completed cycle reads as a record before
  // any filter is considered.
  const storedMode = entity.scope === "cycle" ? "stored" : "visit";
  const filtered =
    !resolvedCompletedCycle &&
    isFilteredEmpty({
      mode: storedMode,
      hasCompleteCondition: parseRichExpression(richFilters).some(isConditionComplete),
      addedThisVisit: false,
      visibleCount: firstPage.total_count,
    });
  const renderWithFilters = (filters: RichFilterBuilderParts | null): React.ReactElement => {
    // The visit-scoped rule also applies here once a condition is added
    // through the row, so the card follows the same predicate as the
    // project list.
    const filteredNow =
      filtered ||
      (!resolvedCompletedCycle &&
        filters !== null &&
        isFilteredEmpty({
          mode: "visit",
          hasCompleteCondition: filters.hasCompleteCondition,
          addedThisVisit: filters.addedThisVisit,
          visibleCount: firstPage.total_count,
        }));
    return (
      <IssuesLayoutRoot
        workspaceSlug={workspaceSlug}
        entity={entity}
        title={resolvedTitle}
        totalCount={firstPage.total_count}
        resultCount={resultCount}
        offered={offered}
        renderLayout={(layout, prefs) =>
          layout === "calendar" ? (
            <CalendarEntry
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              projectIdentifier={project.identifier}
              entity={entity}
              canEdit={canEdit}
              canAddExisting
              membership={{
                ...(entity.scope === "cycle" ? { cycleId: entity.cycleId } : {}),
                ...(entity.scope === "module" ? { moduleId: entity.moduleId } : {}),
              }}
            />
          ) : (
            <ListLayout
              workspaceSlug={workspaceSlug}
              entity={entity}
              projectId={projectId}
              projectIdentifier={project.identifier}
              prefs={prefs}
              canEdit={canEdit}
              richFilters={richFilters}
              cycleId={cycleId}
              moduleId={moduleId}
              completedCycle={resolvedCompletedCycle}
              completedModule={resolvedCompletedModule}
              peekKey={peekKey}
              onPeekChange={onPeekChange}
              compact={compact}
              onRegisterOverlays={setOverlays}
            />
          )
        }
        emptyState={emptyCard(filteredNow, filters)}
        noMatchState={emptyCard(filteredNow, filters)}
        headerActions={display}
        displayControl={display}
        filterControl={filters?.control}
        filterRow={
          <>
            {completedNote ? (
              <>
                <TransferIssuesPrompt canTransfer={canTransfer} onOpen={() => setTransferOpen(true)} />
                <TransferIssuesDialog
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  cycleId={cycleIdForTransfer}
                  open={transferOpen}
                  onClose={() => setTransferOpen(false)}
                />
              </>
            ) : null}
            {filters?.row}
          </>
        }
      />
    );
  };

  function emptyCard(isFiltered: boolean, filters: RichFilterBuilderParts | null): React.ReactElement {
    const context = entity.scope === "cycle" ? "cycle" : "module";
    if (isFiltered) {
      return (
        <FilteredEmptyCard
          context={context}
          clearLabel="Clear filters"
          canAct={canEdit}
          onClear={() => {
            filters?.clearFilters();
          }}
        />
      );
    }
    if (entity.scope === "cycle") {
      return (
        <CycleIssuesEmpty
          completed={resolvedCompletedCycle}
          canAct={canEdit}
          onCreate={() => {
            overlays?.openCreate();
          }}
          onAddExisting={() => {
            overlays?.openAddExisting();
          }}
        />
      );
    }
    return (
      <ModuleIssuesEmpty
        canAct={canEdit}
        onCreate={() => {
          overlays?.openCreate();
        }}
        onAddExisting={() => {
          overlays?.openAddExisting();
        }}
      />
    );
  }

  return (
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
  );
}
