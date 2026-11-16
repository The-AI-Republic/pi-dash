// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Read-only issue list (first vertical slice). Virtualized rows over the
// prefetched list query; the layout search param only changes density.
// Fresh rows mount in an unvirtualized strip above the list so they render
// immediately (parity/specs/issues.md §1, ISS-006); the strip is capped
// and overflow fresh rows virtualize (bug NEWFRONT-1014); temp rows carry
// a pulsing overlay until the server confirms. Selection, filters and
// detail arrive with NEWFRONT-71.

import { Badge, cn, VirtualList } from "@pidash/kit";
import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";

import type { IssueLayout } from "../filters/search.js";
import { isRowHighlighted, isTempRowId, partitionFresh, rowDomId } from "../freshness/fresh.js";

export interface IssueListProps {
  issues: IssueListItem[];
  /** Short code of the owning project, e.g. "WEB". */
  projectIdentifier: string;
  layout: IssueLayout;
  /** Explicitly highlighted row id (drops); fresh rows highlight themselves. */
  highlightedId?: string | null;
}

const rowHeights: Record<IssueLayout, number> = {
  list: 44,
  compact: 32,
};

function issueKey(projectIdentifier: string, sequenceId: number): string {
  return `${projectIdentifier}-${sequenceId}`;
}

function PriorityBadge({ priority }: { priority: IssueListItem["priority"] }): React.ReactElement | null {
  if (priority === "none") return null;
  const tone = priority === "urgent" ? "danger" : priority === "high" ? "warning" : "neutral";
  return <Badge tone={tone}>{priority}</Badge>;
}

interface IssueRowProps {
  issue: IssueListItem;
  projectIdentifier: string;
  roomy: boolean;
  highlighted: boolean;
}

function IssueRow({ issue, projectIdentifier, roomy, highlighted }: IssueRowProps): React.ReactElement {
  const temp = isTempRowId(issue.id);
  return (
    <article
      id={rowDomId(issue.id)}
      aria-label={temp ? `${issue.name}, saving` : issueKey(projectIdentifier, issue.sequence_id)}
      data-highlight={highlighted ? "true" : undefined}
      data-temprow={temp ? "true" : undefined}
      className={cn(
        roomy
          ? "flex h-11 items-center gap-(--space-3) border-b border-(--border) px-(--space-4)"
          : "flex h-8 items-center gap-(--space-2) border-b border-(--border) px-(--space-4)",
        highlighted && "bg-(--accent-subtle)",
        temp && "relative"
      )}
    >
      <span className="text-caption shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)">
        {temp ? "…" : issueKey(projectIdentifier, issue.sequence_id)}
      </span>
      <span className="text-body min-w-0 flex-1 truncate text-(--text)">{issue.name}</span>
      <PriorityBadge priority={issue.priority} />
      {temp ? (
        <span aria-hidden="true" className="pointer-events-none absolute inset-0 animate-pulse bg-(--subtle)/60" />
      ) : null}
    </article>
  );
}

export function IssueList({
  issues,
  projectIdentifier,
  layout,
  highlightedId = null,
}: IssueListProps): React.ReactElement {
  if (issues.length === 0) {
    return (
      <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">No issues yet</h2>
        <p className="text-body text-(--text-muted)">New issues in this project will appear here.</p>
      </section>
    );
  }
  const roomy = layout === "list";
  const { fresh, rest } = partitionFresh(issues);
  return (
    <div className="flex h-full flex-col">
      {fresh.length > 0 ? (
        <div data-testid="issues-fresh-strip" className="shrink-0">
          {fresh.map((issue) => (
            <IssueRow
              key={issue.id}
              issue={issue}
              projectIdentifier={projectIdentifier}
              roomy={roomy}
              highlighted={isRowHighlighted(issue, highlightedId)}
            />
          ))}
        </div>
      ) : null}
      <VirtualList
        label="Issues"
        items={rest}
        estimateSize={rowHeights[layout]}
        getKey={(issue) => issue.id}
        overscan={8}
        className="h-full"
        renderRow={(issue) => (
          <IssueRow
            issue={issue}
            projectIdentifier={projectIdentifier}
            roomy={roomy}
            highlighted={isRowHighlighted(issue, highlightedId)}
          />
        )}
      />
    </div>
  );
}
