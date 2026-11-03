// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The board's content area (SPC-023–026). One wrapper owns the empty test so
// the two layouts cannot disagree: when the fetch's `total_count` is zero,
// neither layout renders its own surface — just the one centred notice. The
// navbar above stays live, so the visitor can still switch layouts or undo a
// filter that emptied the board.

import { EmptyState } from "@pidash/kit";
import type {
  ApiClient,
  SpaceBoardIssue,
  SpaceBoardIssuePage,
  SpaceBoardLabel,
  SpaceBoardState,
} from "@pidash/api-client";
import * as React from "react";

import { SPACE_LANDING_HREF, type SpaceBoardFilters, type SpaceLayout } from "../lib/address.js";
import { SpaceBoardLayout } from "./SpaceBoardLayout.js";
import { SpaceListLayout } from "./SpaceListLayout.js";
import { useSpaceGroups } from "./useSpaceGroups.js";

/** The heading the empty board shows, in both layouts. */
export const SPACE_BOARD_EMPTY_TEXT = "No published work items to show";

/**
 * The one notice either layout falls back to. It names the board's landing
 * as the way out, since a visitor who followed a deep link has no other
 * route back to the list of public boards.
 */
export function EmptyBoardNotice(): React.ReactElement {
  return (
    <div
      data-testid="space-board-empty"
      role="status"
      className="flex size-full flex-col items-center justify-center gap-(--space-4) p-(--space-8)"
    >
      <EmptyState
        variant="illustrated"
        title={SPACE_BOARD_EMPTY_TEXT}
        description="There is nothing published here right now. If you filtered the board, clearing a filter may bring items back."
        art={{ category: "general", type: "empty" }}
      />
      <a
        href={SPACE_LANDING_HREF}
        className="text-meta rounded-(--radius-control) font-medium text-(--accent) hover:underline focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-1 focus-visible:outline-none"
      >
        Back to all public boards
      </a>
    </div>
  );
}

export interface BoardLayoutsProps {
  client: ApiClient;
  anchor: string;
  layout: SpaceLayout;
  filters: SpaceBoardFilters;
  states: readonly SpaceBoardState[];
  page: SpaceBoardIssuePage;
  projectIdentifier: string;
  labels: readonly SpaceBoardLabel[];
  openIssueId?: string | undefined;
  onOpenIssue?: ((issue: SpaceBoardIssue) => void) | undefined;
  issueHref: (issue: SpaceBoardIssue) => string;
  votesEnabled?: boolean;
  reactionsEnabled?: boolean;
  onVote?: ((issue: SpaceBoardIssue, vote: 1 | -1) => void) | undefined;
  onReact?: ((issue: SpaceBoardIssue, reaction: string) => void) | undefined;
  viewerId?: string | null | undefined;
}

export function BoardLayouts({
  client,
  anchor,
  layout,
  filters,
  states,
  page,
  projectIdentifier,
  labels,
  openIssueId,
  onOpenIssue,
  issueHref,
  votesEnabled = false,
  reactionsEnabled = false,
  onVote,
  onReact,
  viewerId,
}: BoardLayoutsProps): React.ReactElement {
  const { groups, paging, failed, loadMore } = useSpaceGroups(client, anchor, filters, states, page);
  if (page.total_count === 0) return <EmptyBoardNotice />;
  return layout === "kanban" ? (
    <SpaceBoardLayout
      groups={groups}
      paging={paging}
      failed={failed}
      onLoadMore={loadMore}
      projectIdentifier={projectIdentifier}
      labels={labels}
      issueHref={issueHref}
      openIssueId={openIssueId}
      onOpenIssue={onOpenIssue}
      votesEnabled={votesEnabled}
      reactionsEnabled={reactionsEnabled}
      onVote={onVote}
      onReact={onReact}
      viewerId={viewerId}
    />
  ) : (
    <SpaceListLayout
      groups={groups}
      paging={paging}
      failed={failed}
      onLoadMore={loadMore}
      projectIdentifier={projectIdentifier}
      labels={labels}
      issueHref={issueHref}
      openIssueId={openIssueId}
      onOpenIssue={onOpenIssue}
    />
  );
}
