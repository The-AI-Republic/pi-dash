// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Read-only issue list (first vertical slice). Virtualized rows over the
// prefetched list query; the layout search param only changes density.
// Selection, filters and detail arrive with NEWFRONT-71.

import { Badge, VirtualList } from "@pidash/kit";
import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";

import type { IssueLayout } from "../filters/search.js";

export interface IssueListProps {
  issues: IssueListItem[];
  /** Short code of the owning project, e.g. "WEB". */
  projectIdentifier: string;
  layout: IssueLayout;
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

export function IssueList({ issues, projectIdentifier, layout }: IssueListProps): React.ReactElement {
  if (issues.length === 0) {
    return (
      <section aria-label="Issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">No issues yet</h2>
        <p className="text-body text-(--text-muted)">New issues in this project will appear here.</p>
      </section>
    );
  }
  const roomy = layout === "list";
  return (
    <VirtualList
      label="Issues"
      items={issues}
      estimateSize={rowHeights[layout]}
      getKey={(issue) => issue.id}
      overscan={8}
      className="h-full"
      renderRow={(issue) => (
        <article
          aria-label={issueKey(projectIdentifier, issue.sequence_id)}
          className={
            roomy
              ? "flex h-11 items-center gap-(--space-3) border-b border-(--border) px-(--space-4)"
              : "flex h-8 items-center gap-(--space-2) border-b border-(--border) px-(--space-4)"
          }
        >
          <span className="text-caption shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)">
            {issueKey(projectIdentifier, issue.sequence_id)}
          </span>
          <span className="text-body min-w-0 flex-1 truncate text-(--text)">{issue.name}</span>
          <PriorityBadge priority={issue.priority} />
        </article>
      )}
    />
  );
}
