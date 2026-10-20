// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived work items surface (parity/specs/issues.md §8, ISS-071).
// Thin by design: the archives slices own the tab shell, the read-only
// list, the display control and the row menus (NEWFRONT-698–700). This
// screen mounts the row ISS-071 names at the address the oracle opens —
// the card when nothing is archived and the no-results card once a
// condition was added through this page's filter row — over the archived
// read and the page's own window-local expression (NEWFRONT-167).
//
// The row runs in temporary mode: the archived page never reads or writes
// server preferences, so edits live in memory and in the local store for
// the next visit to restore.

import { useSuspenseQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { usePermissions } from "../../../core/session/hooks.js";
import { IssueList } from "../components/IssueList.js";
import { ArchivedIssuesEmpty, FilteredEmptyCard } from "../emptyStates/cards.js";
import { isFilteredEmpty } from "../emptyStates/filteredEmpty.js";
import { parseRichExpression, serializeRichExpression } from "../filters/rich/expression.js";
import { useRichFilterBuilder } from "../filters/rich/RichFilterBuilder.js";
import { projectsQueryOptions } from "../../projects/index.js";
import {
  archivedIssuesQueryOptions,
  projectAutomationsPath,
  readArchivedLocalFilter,
  writeArchivedLocalFilter,
} from "./archivedIssues.js";

/**
 * The archived list's order. The page owns its own display state and the
 * archives slice (NEWFRONT-699) takes the control over; this host keeps
 * the read's server default so it never touches the server preference
 * record (NEWFRONT-167).
 */
export const ARCHIVED_ORDER = "-created_at";

export interface ArchivedIssuesScreenProps {
  workspaceSlug: string;
  projectId: string;
}

/** The archived tab body: one read, one local expression, one card. */
export function ArchivedIssuesScreen({ workspaceSlug, projectId }: ArchivedIssuesScreenProps): React.ReactElement {
  const navigate = useNavigate();
  const client = getAppClient();
  const { canEdit } = usePermissions(workspaceSlug);
  const { data: projects } = useSuspenseQuery(projectsQueryOptions(client, workspaceSlug));
  const project = projects.find((entry) => entry.id === projectId) ?? null;
  // The local record seeds the row once per visit; the row owns edits after
  // that, so a refetch never rewrites what the user is typing.
  const [seed] = React.useState(() => parseRichExpression(readArchivedLocalFilter(workspaceSlug, projectId)));
  const parts = useRichFilterBuilder({
    workspaceSlug,
    projectId,
    entity: { scope: "project", projectId },
    hideCycle: !(project?.cycle_view ?? false),
    hideModule: !(project?.module_view ?? false),
    temporary: true,
    temporarySeed: seed,
  });
  const expression = React.useMemo(() => serializeRichExpression(parts.conditions), [parts.conditions]);
  React.useEffect(() => {
    writeArchivedLocalFilter(workspaceSlug, projectId, expression);
  }, [expression, projectId, workspaceSlug]);
  const { data: archived } = useSuspenseQuery(
    archivedIssuesQueryOptions(client, workspaceSlug, projectId, expression, ARCHIVED_ORDER)
  );

  if (project === null) {
    return (
      <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">Project not found</h2>
        <p className="text-body text-(--text-muted)">Pick another project from the sidebar.</p>
      </section>
    );
  }

  // A stored expression on its own keeps the first-run card: the archived
  // list changes card only once a condition was added through the row
  // during this visit (spec §8). Clearing puts the unfiltered list back.
  const filtered = isFilteredEmpty({
    mode: "visit",
    hasCompleteCondition: parts.hasCompleteCondition,
    addedThisVisit: parts.addedThisVisit,
    visibleCount: archived.total_count,
  });
  return (
    <div data-testid="archived-issues" className="flex h-full flex-col">
      <div className="flex shrink-0 items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-3)">
        <h2 className="text-h3 text-(--text)">Archived work items</h2>
        <span className="text-body text-(--text-muted)">
          {archived.total_count} issue{archived.total_count === 1 ? "" : "s"}
        </span>
        <div className="ml-auto flex items-center gap-(--space-2)">{parts.control}</div>
      </div>
      {parts.row}
      <div className="min-h-0 flex-1">
        {filtered ? (
          <FilteredEmptyCard
            context="archived"
            clearLabel="Clear filters"
            canAct={canEdit}
            onClear={parts.clearFilters}
          />
        ) : archived.total_count === 0 ? (
          <ArchivedIssuesEmpty
            canAct={canEdit}
            onSetAutomation={() => {
              void navigate({ to: projectAutomationsPath(workspaceSlug, projectId) } as never);
            }}
          />
        ) : (
          <IssueList issues={archived.results} projectIdentifier={project.identifier} layout="list" />
        )}
      </div>
    </div>
  );
}
