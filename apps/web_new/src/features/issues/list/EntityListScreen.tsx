// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// List layout host for cycle and module pages (parity/specs/issues.md §2,
// ISS-007–014). The cycles and modules areas own those pages; the issues
// slice only mounts the same layout root and list renderer the project
// route uses, so grouping, creation and the inline strip behave the same
// in every issues-bearing context.

import { useSuspenseQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { DisplayControl } from "../display/DisplayControl.js";
import type { LayoutKind } from "../layouts/kinds.js";
import { IssuesLayoutRoot } from "../layouts/IssuesLayoutRoot.js";
import { prefsQueryOptions } from "../prefs/prefsQuery.js";
import { listPageQueryOptions } from "./pages.js";
import { resolvedListLayout } from "./lazy.js";
import type { IssuesEntity } from "../prefs/entities.js";

export interface EntityListScreenProps {
  workspaceSlug: string;
  projectId: string;
  entity: IssuesEntity;
  /** Page title, e.g. the cycle or module name. */
  title: string;
  cycleId?: string | undefined;
  moduleId?: string | undefined;
  /** Completed cycles render read-only. */
  completedCycle?: boolean | undefined;
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
  peekKey,
  onPeekChange,
  compact,
}: EntityListScreenProps): React.ReactElement {
  const client = getAppClient();
  const { data: projects } = useSuspenseQuery(projectsQueryOptions(client, workspaceSlug));
  const project = projects.find((row) => row.id === projectId) ?? null;
  // Stored rich filters scope the list in cycle/module context too.
  const { data: preferences } = useSuspenseQuery(prefsQueryOptions(client, workspaceSlug, entity));
  const richFilters = preferences.rich_filters;
  const { data: firstPage } = useSuspenseQuery(
    listPageQueryOptions(client, workspaceSlug, { entity, groupBy: null, orderBy: "sort_order", richFilters })
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
  const offered: readonly LayoutKind[] = ["list"];
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
      title={title}
      totalCount={firstPage.total_count}
      resultCount={Array.isArray(firstPage.results) ? firstPage.results.length : firstPage.count}
      offered={offered}
      renderLayout={(_layout, prefs) => (
        <ListLayout
          workspaceSlug={workspaceSlug}
          entity={entity}
          projectId={projectId}
          projectIdentifier={project.identifier}
          prefs={prefs}
          richFilters={richFilters}
          cycleId={cycleId}
          moduleId={moduleId}
          completedCycle={completedCycle}
          peekKey={peekKey}
          onPeekChange={onPeekChange}
          compact={compact}
        />
      )}
      emptyState={<EntityEmptyState title={title} />}
      noMatchState={<EntityEmptyState title={title} />}
      headerActions={display}
      displayControl={display}
    />
  );
}
