// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Recent-activity widget (SHELL-014–017; written from parity/specs/shell.md
// §2 and §11). The feed of recently visited work items, pages and projects
// in visit order with a per-type filter in its header. Work-item rows open
// the shared peek panel in place; page and project rows navigate. Empty
// slices show their own art, loading shows the widget's skeleton shape,
// and a failed fetch keeps prior rows with no retry affordance.

import { useQueries } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { FileText, Folder, History, SquareKanban } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import * as React from "react";

import { Button, Skeleton } from "@pidash/kit";
import type { ProjectList } from "@pidash/api-client";
import { getAppClient } from "../../../core/api/client.js";
import { PeekPanel } from "../../../shared/shell/index.js";
import { usePreviewRename } from "../api/previewRename.js";
import { projectStatesQueryOptions } from "../api/projectStates.js";
import { useRecentVisits } from "../api/recents.js";
import { formatRelativeVisitTime } from "../lib/relativeTime.js";
import {
  indexProjectIdentifiers,
  indexStateLists,
  issueKeyLabel,
  parseRecentRows,
  RECENT_FILTERS,
  recentProjectIds,
  resolveIssueState,
  type RecentFilter,
  type RecentIssueData,
} from "../lib/recentRows.js";
import { RecentIssueRow, RecentPageRow, RecentProjectRow } from "./RecentRows.js";

/** Rows before the feed caps behind the expander. */
const COLLAPSED_ROWS = 5;

// Pure data: icon components (not elements) so the module stays
// side-effect-free and tree-shakes out of route chains that only use the
// feature's query options (performance budget: initial JS ≤ 220 KB).
const EMPTY_ART: Record<RecentFilter, { icon: LucideIcon; heading: string; body: string }> = {
  all: {
    icon: History,
    heading: "No recent activity",
    body: "Pages, projects and work items you open will show up here.",
  },
  issue: {
    icon: SquareKanban,
    heading: "No recently viewed work items",
    body: "Work items you open will show up here.",
  },
  page: {
    icon: FileText,
    heading: "No recently viewed pages",
    body: "Pages you open will show up here.",
  },
  project: {
    icon: Folder,
    heading: "No recently viewed projects",
    body: "Projects you open will show up here.",
  },
};

function RecentsEmptyState({ filter }: { filter: RecentFilter }): React.ReactElement {
  const art = EMPTY_ART[filter];
  const Icon = art.icon;
  return (
    <div
      data-testid="recents-empty"
      className="flex flex-col items-center gap-(--space-2) px-(--space-4) py-(--space-8) text-center"
    >
      <span
        aria-hidden="true"
        className="grid size-10 place-items-center rounded-(--radius-full) bg-(--subtle) text-(--text-muted)"
      >
        <Icon size={20} strokeWidth={1.5} aria-hidden="true" />
      </span>
      <p className="text-emphasis text-(--text)">{art.heading}</p>
      <p className="text-body text-(--text-muted)">{art.body}</p>
    </div>
  );
}

function WorkItemPreview({
  workspaceSlug,
  issue,
  visitedAt,
  stateName,
  keyLabel,
  onClose,
}: {
  workspaceSlug: string;
  issue: RecentIssueData;
  visitedAt: string;
  stateName: string | null;
  keyLabel: string | null;
  onClose: () => void;
}): React.ReactElement {
  const rename = usePreviewRename(workspaceSlug, issue.projectId ?? "", issue.issueId);
  const context = [stateName, formatRelativeVisitTime(visitedAt)]
    .filter((part): part is string => part !== null)
    .join(" · ");
  return (
    <PeekPanel
      eyebrow={keyLabel}
      title={issue.title}
      titleLabel="Work item title"
      onClose={onClose}
      onCommitTitle={issue.projectId === null ? undefined : (next: string) => rename.mutate(next)}
    >
      <p className="text-body text-(--text-muted)">{context}</p>
    </PeekPanel>
  );
}

export function RecentsWidget({
  workspaceSlug,
  projects,
}: {
  workspaceSlug: string;
  /** Workspace projects for identifier fallbacks; the route reads them. */
  projects: ProjectList | undefined;
}): React.ReactElement {
  const [filter, setFilter] = React.useState<RecentFilter>("all");
  const [expanded, setExpanded] = React.useState(false);
  const [preview, setPreview] = React.useState<{ issue: RecentIssueData; visitedAt: string } | null>(null);
  const navigate = useNavigate();
  const visits = useRecentVisits(workspaceSlug, filter);

  const rows = React.useMemo(() => parseRecentRows(visits.data ?? []), [visits.data]);
  const projectIds = React.useMemo(() => recentProjectIds(rows), [rows]);
  const statesReads = useQueries({
    queries: projectIds.map((projectId) => projectStatesQueryOptions(getAppClient(), workspaceSlug, projectId)),
  });
  const statesById = React.useMemo(() => indexStateLists(statesReads.map((read) => read.data)), [statesReads]);
  const identifiersById = React.useMemo(() => indexProjectIdentifiers(projects), [projects]);
  const identifierOf = (projectId: string | null): string | null =>
    projectId === null ? null : (identifiersById.get(projectId) ?? null);
  const openHref = (href: string): void => {
    void navigate({ to: href });
  };

  return (
    <section
      id="home-recents"
      aria-label="Recents"
      data-widget-section="recents"
      data-widget-title="Recents"
      className="flex flex-col gap-(--space-3)"
    >
      <div className="flex items-center justify-between gap-(--space-3)">
        <h2 className="text-emphasis text-(--text)">Recents</h2>
        {/* Native dropdown: the floating kit Menu would pull its popup
            runtime into the initial route chain (performance budget:
            initial JS at most 220 KB gzipped). */}
        <select
          aria-label="Filter recents"
          value={filter}
          onChange={(event) => {
            const next = RECENT_FILTERS.find((entry) => entry.key === event.target.value)?.key ?? "all";
            setFilter(next);
            setExpanded(false);
          }}
          className="text-body rounded-(--radius-control) border border-(--border) bg-(--surface) px-(--space-3) py-(--space-1) text-(--text)"
        >
          {RECENT_FILTERS.map((entry) => (
            <option key={entry.key} value={entry.key}>
              {entry.label}
            </option>
          ))}
        </select>
      </div>
      {visits.isPending ? (
        <div data-testid="recents-skeleton" aria-busy="true" className="flex flex-col gap-(--space-2)">
          <Skeleton height="44px" />
          <Skeleton height="44px" />
          <Skeleton height="44px" />
        </div>
      ) : rows.length > 0 ? (
        <>
          <div className={expanded ? undefined : "max-h-[26rem] overflow-y-auto"}>
            <ul aria-label="Recently visited" className="flex flex-col">
              {rows.map((row) => {
                if (row.entity.kind !== "issue") {
                  return (
                    <li key={row.visitId}>
                      {row.entity.kind === "page" ? (
                        <RecentPageRow
                          workspaceSlug={workspaceSlug}
                          visitedAt={row.visitedAt}
                          page={row.entity}
                          onOpen={openHref}
                        />
                      ) : (
                        <RecentProjectRow
                          workspaceSlug={workspaceSlug}
                          visitedAt={row.visitedAt}
                          project={row.entity}
                          onOpen={openHref}
                        />
                      )}
                    </li>
                  );
                }
                const issue = row.entity;
                const state = resolveIssueState(issue, statesById);
                return (
                  <li key={row.visitId}>
                    <RecentIssueRow
                      workspaceSlug={workspaceSlug}
                      visitedAt={row.visitedAt}
                      issue={issue}
                      stateName={state.name}
                      stateColor={state.color}
                      fallbackIdentifier={identifierOf(issue.projectId)}
                      onPreview={() => {
                        setPreview({ issue, visitedAt: row.visitedAt });
                      }}
                    />
                  </li>
                );
              })}
            </ul>
          </div>
          {rows.length > COLLAPSED_ROWS ? (
            <Button variant="secondary" size="small" onClick={() => setExpanded((value) => !value)}>
              {expanded ? "Show less" : "Show all"}
            </Button>
          ) : null}
        </>
      ) : visits.isError ? null : (
        <RecentsEmptyState filter={filter} />
      )}
      {preview === null ? null : (
        <WorkItemPreview
          workspaceSlug={workspaceSlug}
          issue={preview.issue}
          visitedAt={preview.visitedAt}
          stateName={resolveIssueState(preview.issue, statesById).name}
          keyLabel={issueKeyLabel(preview.issue, identifierOf(preview.issue.projectId))}
          onClose={() => {
            setPreview(null);
          }}
        />
      )}
    </section>
  );
}
