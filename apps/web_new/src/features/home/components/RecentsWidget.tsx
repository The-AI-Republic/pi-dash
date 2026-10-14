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
import { ChevronDown, FileText, Folder, History, SquareKanban } from "lucide-react";
import * as React from "react";

import { Button, Menu, Skeleton } from "@pidash/kit";
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
  RECENT_FILTER_LABELS,
  RECENT_FILTERS,
  recentProjectIds,
  resolveIssueState,
  type RecentFilter,
  type RecentIssueData,
} from "../lib/recentRows.js";
import { RecentIssueRow, RecentPageRow, RecentProjectRow } from "./RecentRows.js";

/** Rows before the feed caps behind the expander. */
const COLLAPSED_ROWS = 5;

const EMPTY_ART: Record<RecentFilter, { icon: React.ReactNode; heading: string; body: string }> = {
  all: {
    icon: <History size={20} strokeWidth={1.5} aria-hidden="true" />,
    heading: "No recent activity",
    body: "Pages, projects and work items you open will show up here.",
  },
  issue: {
    icon: <SquareKanban size={20} strokeWidth={1.5} aria-hidden="true" />,
    heading: "No recently viewed work items",
    body: "Work items you open will show up here.",
  },
  page: {
    icon: <FileText size={20} strokeWidth={1.5} aria-hidden="true" />,
    heading: "No recently viewed pages",
    body: "Pages you open will show up here.",
  },
  project: {
    icon: <Folder size={20} strokeWidth={1.5} aria-hidden="true" />,
    heading: "No recently viewed projects",
    body: "Projects you open will show up here.",
  },
};

function RecentsEmptyState({ filter }: { filter: RecentFilter }): React.ReactElement {
  const art = EMPTY_ART[filter];
  return (
    <div
      data-testid="recents-empty"
      className="flex flex-col items-center gap-(--space-2) px-(--space-4) py-(--space-8) text-center"
    >
      <span
        aria-hidden="true"
        className="grid size-10 place-items-center rounded-(--radius-full) bg-(--subtle) text-(--text-muted)"
      >
        {art.icon}
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

  const activeLabel = RECENT_FILTER_LABELS[filter];

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
        <Menu
          trigger={
            <Button variant="secondary" size="small" aria-label={`Filter recents, showing ${activeLabel}`}>
              <span>{activeLabel}</span>
              <ChevronDown size={14} strokeWidth={1.5} aria-hidden="true" />
            </Button>
          }
          items={RECENT_FILTERS.map((entry) => ({
            label: entry.label,
            onSelect: () => {
              setFilter(entry.key);
              setExpanded(false);
            },
          }))}
        />
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
