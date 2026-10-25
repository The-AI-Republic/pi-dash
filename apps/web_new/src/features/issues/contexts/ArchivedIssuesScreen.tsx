// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived work-items tab body (parity/specs/archives.md §2,
// ARCH-005–007). A read-only list with the shared rich-filter row and
// display control, grouped client-side by the server's buckets, and the
// archived side peek driven by the address. The tab's expression,
// display options and columns live in the window-local record the
// oracle seeds; the project's server preferences are never read or
// written here. Row menus, restore/delete and the empty-state
// automation control belong to NEWFRONT-700.

import { useQuery, useSuspenseQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Skeleton } from "@pidash/kit";
import type { IssueListItem, ProjectDetail } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { canEditInProject } from "../../../shared/shell/contextual/index.js";
import { DisplayControl } from "../display/DisplayControl.js";
import { archivedDisplayMatrix } from "../display/matrix.js";
import { ArchivedIssuesEmpty, FilteredEmptyCard } from "../emptyStates/cards.js";
import { isFilteredEmpty } from "../emptyStates/filteredEmpty.js";
import { parseRichExpression, serializeRichExpression } from "../filters/rich/expression.js";
import { useRichFilterBuilder } from "../filters/rich/RichFilterBuilder.js";
import { GroupHeader } from "../list/GroupHeader.js";
import { ListRow } from "../list/ListRow.js";
import { useListReferences } from "../list/reference.js";
import type { DisplayPrefs } from "../prefs/displayPrefs.js";
import { projectDetailsQueryOptions, projectsQueryOptions } from "../../projects/index.js";
import { ArchivedIssuePeek } from "../peek/ArchivedIssuePeek.js";
import {
  projectAutomationsPath,
  readArchivedLocalState,
  useArchivedPages,
  writeArchivedLocalState,
  type ArchivedLocalState,
} from "./archivedIssues.js";

/** The archived list always offers the list layout. */
const ARCHIVED_LAYOUTS = ["list"] as const;

/** Read-only sections carry no add or collapse behavior. */
const NOOP = (): void => undefined;

export interface ArchivedIssuesScreenProps {
  workspaceSlug: string;
  projectId: string;
  /** Peeked archived item from ?peekIssueId=; null when no peek is open. */
  peekIssueId?: string | null | undefined;
  /** Writes the peek selection into the address (null clears it). */
  onPeekChange?: ((issueId: string | null) => void) | undefined;
}

function archivedHref(workspaceSlug: string, projectId: string, issue: IssueListItem): string {
  return `/${workspaceSlug}/projects/${projectId}/archives/issues/${issue.id}`;
}

/** The archived tab body. */
export function ArchivedIssuesScreen({
  workspaceSlug,
  projectId,
  peekIssueId = null,
  onPeekChange,
}: ArchivedIssuesScreenProps): React.ReactElement {
  const navigate = useNavigate();
  const client = getAppClient();
  const { data: projects } = useSuspenseQuery(projectsQueryOptions(client, workspaceSlug));
  const { data: details } = useQuery(projectDetailsQueryOptions(client, workspaceSlug));
  const project = projects.find((entry) => entry.id === projectId) ?? null;
  const projectDetails: ProjectDetail | null = details?.find((entry) => entry.id === projectId) ?? null;

  // The local record seeds the row once per visit; the row owns edits
  // afterwards, so a refetch never rewrites what the user is typing.
  const [seed] = React.useState<ArchivedLocalState>(() => readArchivedLocalState(workspaceSlug, projectId));
  const parts = useRichFilterBuilder({
    workspaceSlug,
    projectId,
    entity: { scope: "project", projectId },
    hideCycle: !(project?.cycle_view ?? false),
    hideModule: !(project?.module_view ?? false),
    temporary: true,
    temporarySeed: parseRichExpression(seed.expression),
  });
  const [display, setDisplay] = React.useState<DisplayPrefs>(seed.display);
  const expression = React.useMemo(() => serializeRichExpression(parts.conditions), [parts.conditions]);
  const refs = useListReferences({ workspaceSlug, projectId, dimension: display.groupBy });
  const labels = refs.labels[display.groupBy ?? ""] ?? new Map<string, string>();
  const pages = useArchivedPages(
    workspaceSlug,
    projectId,
    expression,
    display.groupBy,
    display.orderBy,
    labels,
    display.showEmptyGroups
  );

  // Persist both halves together, preserving unknown members. The writer
  // leaves a pristine page with no entry at all.
  React.useEffect(() => {
    writeArchivedLocalState(workspaceSlug, projectId, expression, display);
  }, [display, expression, projectId, workspaceSlug]);

  if (project === null) {
    return (
      <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">Project not found</h2>
        <p className="text-body text-(--text-muted)">Pick another project from the sidebar.</p>
      </section>
    );
  }

  const projectIdentifier = project.identifier;
  const canEdit = canEditInProject({ memberRole: project.member_role, archived: project.archived_at !== null });
  const filtered = isFilteredEmpty({
    // Archived expressions come from the stored record, so a restored
    // condition that hides every row must show the no-match card even
    // though this visit did not add it (spec §2.4, ARCH-033).
    mode: "stored",
    hasCompleteCondition: parts.hasCompleteCondition,
    addedThisVisit: parts.addedThisVisit,
    visibleCount: pages.total,
  });
  const estimateEnabled = projectDetails !== null && projectDetails.estimate !== null;
  const flags = { hideCycle: !project.cycle_view, hideModule: !project.module_view };

  // The peek composes the list row with the archive-scoped detail read
  // (spec §2.5): the detail serializer omits the membership and count
  // annotations only the list carries, so hand the peek its row.
  const peekRow =
    peekIssueId === null
      ? null
      : (pages.sections.flatMap((section) => section.rows).find((row) => row.id === peekIssueId) ?? null);

  const openPeek = (issueId: string): void => {
    onPeekChange?.(issueId === peekIssueId ? null : issueId);
  };

  function renderRow(issue: IssueListItem): React.ReactElement {
    return (
      <ListRow
        key={issue.id}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectIdentifier={projectIdentifier}
        issue={issue}
        depth={1}
        properties={display.properties}
        flags={flags}
        readOnly
        states={refs.stateRows}
        stateNames={refs.stateNames}
        assigneeNames={refs.assigneeNames}
        labelNames={refs.labelNames}
        cycleNames={refs.cycleNames}
        moduleNames={refs.moduleNames}
        openCycleIds={refs.openCycleIds}
        estimateEnabled={estimateEnabled}
        highlightedId={null}
        onOpenPeek={openPeek}
        hrefFor={(row) => archivedHref(workspaceSlug, projectId, row)}
        showSubIssues={false}
        timeZone={projectDetails?.timezone ?? null}
        menuContext="archived"
        showMenu={false}
      />
    );
  }

  return (
    <div data-testid="archived-issues" className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 flex-wrap items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-3)">
        <h2 className="text-h3 text-(--text)">Archived work items</h2>
        <span className="text-body text-(--text-muted)">
          {pages.total} issue{pages.total === 1 ? "" : "s"}
        </span>
        <div className="ml-auto flex items-center gap-(--space-2)">
          {parts.control}
          <DisplayControl
            workspaceSlug={workspaceSlug}
            entity={{ scope: "project", projectId }}
            offered={ARCHIVED_LAYOUTS}
            hideCycle={flags.hideCycle}
            hideModule={flags.hideModule}
            display={display}
            onDisplayChange={setDisplay}
            matrix={archivedDisplayMatrix()}
          />
        </div>
      </div>
      {parts.row}
      <div className="min-h-0 flex-1 overflow-auto pb-16">
        {pages.isError ? (
          <section role="alert" className="flex flex-col items-start gap-(--space-2) p-(--space-6)">
            <p className="text-body text-(--text)">Couldn't load the archived work items.</p>
            <button
              type="button"
              onClick={pages.refetch}
              className="text-body rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--accent) hover:bg-(--subtle)"
            >
              Try again
            </button>
          </section>
        ) : pages.isPending ? (
          <div data-testid="archived-issues-loading" className="flex flex-col gap-(--space-2) p-(--space-3)">
            {Array.from({ length: 6 }, (_, index) => (
              <Skeleton key={index} height="32px" />
            ))}
          </div>
        ) : filtered ? (
          <FilteredEmptyCard context="archived" clearLabel="Clear filters" canAct onClear={parts.clearFilters} />
        ) : pages.total === 0 ? (
          <ArchivedIssuesEmpty
            canAct={canEdit}
            onSetAutomation={() => {
              void navigate({ to: projectAutomationsPath(workspaceSlug, projectId) } as never);
            }}
          />
        ) : (
          <div className="flex flex-col">
            {pages.sections.map((section) =>
              pages.grouped ? (
                <section key={section.key} className="flex flex-col">
                  <GroupHeader
                    section={section}
                    dimension={display.groupBy}
                    collapsed={false}
                    onToggle={NOOP}
                    canAdd={false}
                    menu={false}
                    onCreate={NOOP}
                    onAddExisting={NOOP}
                  />
                  {section.rows.map((issue) => renderRow(issue))}
                  {pages.sectionMore(section) ? (
                    <button
                      type="button"
                      onClick={() => pages.loadMoreSection(section)}
                      className="text-body border-b border-(--border) px-(--space-3) py-(--space-2) text-left text-(--accent) hover:bg-(--subtle)"
                    >
                      Load more
                    </button>
                  ) : null}
                </section>
              ) : (
                <section key={section.key} className="flex flex-col">
                  {section.rows.map((issue) => renderRow(issue))}
                </section>
              )
            )}
            {pages.hasMore ? (
              <button
                type="button"
                onClick={pages.loadMore}
                className="text-body border-b border-(--border) px-(--space-3) py-(--space-2) text-left text-(--accent) hover:bg-(--subtle)"
              >
                Load more
              </button>
            ) : null}
          </div>
        )}
      </div>
      {peekIssueId === null ? null : (
        <ArchivedIssuePeek
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          projectIdentifier={projectIdentifier}
          issueId={peekIssueId}
          row={peekRow}
          onClose={() => onPeekChange?.(null)}
        />
      )}
    </div>
  );
}
