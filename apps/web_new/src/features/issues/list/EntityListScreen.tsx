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
import type { IssuesEntity } from "../prefs/entities.js";
import { pickerCyclesQueryOptions, pickerModulesQueryOptions } from "../../../shared/pickers/reference.js";
import { isCompletedCycle } from "../../../shared/pickers/cycles.js";
import { isCompletedModule } from "../../../shared/pickers/modules.js";

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

function EntityEmptyState({ title }: { title: string }): React.ReactElement {
  return (
    <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
      <h2 className="text-h3 text-(--text)">No work items in {title}</h2>
      <p className="text-body text-(--text-muted)">Work items added here will appear in this list.</p>
    </section>
  );
}

/** Same layout root and list renderer as the project issue screen. */
export function EntityListScreen({
  workspaceSlug,
  projectId,
  entity,
  title,
  cycleId,
  moduleId,
  completedCycle = false,
  completedModule = false,
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
  return (
    <IssuesLayoutRoot
      workspaceSlug={workspaceSlug}
      entity={entity}
      title={resolvedTitle}
      totalCount={firstPage.total_count}
      resultCount={Array.isArray(firstPage.results) ? firstPage.results.length : firstPage.count}
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
          />
        )
      }
      emptyState={<EntityEmptyState title={resolvedTitle} />}
      noMatchState={<EntityEmptyState title={resolvedTitle} />}
      headerActions={display}
      displayControl={display}
    />
  );
}
