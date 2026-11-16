// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// SPC-023/026: the stacked list. One collapsible section per workflow state
// in board order, each header carrying the state's server count (zero
// included), and a manual "Load more" control under a group only while that
// group has another page. Pressing it swaps the control for placeholder rows
// until the page lands, then appends the rows and retires the control when
// the group is exhausted. Collapsing keeps a group's paging state.

import * as React from "react";

import type { SpaceBoardIssue, SpaceBoardLabel } from "@pidash/api-client";

import type { SpaceIssueGroup } from "./model.js";
import { groupHasNextPage } from "./model.js";
import type { SpaceGroupsState } from "./useSpaceGroups.js";
import { GroupHeader } from "./GroupHeader.js";
import { SpaceIssueRow } from "./SpaceIssueRow.js";

export interface SpaceListLayoutProps {
  groups: readonly SpaceIssueGroup[];
  paging: ReadonlySet<string>;
  failed: ReadonlySet<string>;
  onLoadMore: SpaceGroupsState["loadMore"];
  projectIdentifier: string;
  labels: readonly SpaceBoardLabel[];
  issueHref: (issue: SpaceBoardIssue) => string;
  openIssueId?: string | undefined;
  onOpenIssue?: ((issue: SpaceBoardIssue) => void) | undefined;
}

export function SpaceListLayout({
  groups,
  paging,
  failed,
  onLoadMore,
  projectIdentifier,
  labels,
  issueHref,
  openIssueId,
  onOpenIssue,
}: SpaceListLayoutProps): React.ReactElement {
  return (
    <div data-testid="space-list-layout" className="flex flex-col gap-(--space-4) p-(--space-4)">
      {groups.map((group) => (
        <ListGroup
          key={group.stateId}
          group={group}
          paging={paging.has(group.stateId)}
          failed={failed.has(group.stateId)}
          onLoadMore={() => onLoadMore(group.stateId)}
          projectIdentifier={projectIdentifier}
          labels={labels}
          issueHref={issueHref}
          openIssueId={openIssueId}
          onOpenIssue={onOpenIssue}
        />
      ))}
    </div>
  );
}

interface ListGroupProps {
  group: SpaceIssueGroup;
  paging: boolean;
  failed: boolean;
  onLoadMore: () => void;
  projectIdentifier: string;
  labels: readonly SpaceBoardLabel[];
  issueHref: (issue: SpaceBoardIssue) => string;
  openIssueId: string | undefined;
  onOpenIssue: ((issue: SpaceBoardIssue) => void) | undefined;
}

function ListGroup({
  group,
  paging,
  failed,
  onLoadMore,
  projectIdentifier,
  labels,
  issueHref,
  openIssueId,
  onOpenIssue,
}: ListGroupProps): React.ReactElement {
  const [expanded, setExpanded] = React.useState(true);
  const hasNext = groupHasNextPage(group);
  return (
    <section data-testid="space-list-group" data-state-id={group.stateId} className="flex flex-col">
      <GroupHeader
        state={group.state}
        count={group.total}
        expanded={expanded}
        onToggle={() => setExpanded((current) => !current)}
      />
      {expanded ? (
        <div data-testid="space-list-group-body" className="flex flex-col gap-(--space-1) pt-(--space-1)">
          {group.issues.map((issue) => (
            <SpaceIssueRow
              key={issue.id}
              issue={issue}
              state={group.state}
              labels={labels}
              projectIdentifier={projectIdentifier}
              issueHref={issueHref(issue)}
              selected={issue.id === openIssueId}
              onOpenIssue={onOpenIssue}
            />
          ))}
          {paging ? <PlaceholderRows /> : null}
          {hasNext && !paging ? (
            <button
              type="button"
              data-testid="space-list-load-more"
              onClick={onLoadMore}
              className="text-meta mt-(--space-1) flex cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left font-medium text-(--accent) hover:underline"
            >
              {failed ? "Load more — retry" : "Load more"}
            </button>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}

/** Three pulsing rows that hold the group's layout while its page loads. */
export function PlaceholderRows(): React.ReactElement {
  return (
    <div data-testid="space-list-placeholder" aria-hidden="true" className="flex flex-col gap-(--space-1)">
      {[0, 1, 2].map((index) => (
        <div
          key={index}
          className="flex h-10 items-center gap-(--space-3) rounded-(--radius-control) border border-(--border) px-(--space-3)"
        >
          <span className="block h-3 w-12 animate-pulse rounded-(--radius-control) bg-(--subtle)" />
          <span className="block h-3 w-1/2 animate-pulse rounded-(--radius-control) bg-(--subtle)" />
        </div>
      ))}
    </div>
  );
}
