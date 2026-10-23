// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The board's content area (SPC-023–026). One wrapper owns the empty test so
// the two layouts cannot disagree: when the fetch's `total_count` is zero,
// neither layout renders its own surface — just the one centred notice. The
// navbar above stays live, so the visitor can still switch layouts or undo a
// filter that emptied the board.

import { Button, EmptyState } from "@pidash/kit";
import type { ApiClient, SpaceBoardIssuePage, SpaceBoardState } from "@pidash/api-client";
import * as React from "react";

import type { SpaceBoardFilters, SpaceLayout } from "../lib/address.js";
import { SpaceBoardLayout } from "./SpaceBoardLayout.js";
import { SpaceListLayout } from "./SpaceListLayout.js";
import { useSpaceGroups } from "./useSpaceGroups.js";

/** The one line the empty board says, in both layouts. */
export const SPACE_BOARD_EMPTY_TEXT = "No work items found";

export interface EmptyBoardNoticeProps {
  /** Optional way back to the public landing page. */
  onGoHome?: (() => void) | undefined;
}

export function EmptyBoardNotice({ onGoHome }: EmptyBoardNoticeProps): React.ReactElement {
  return (
    <div
      data-testid="space-board-empty"
      role="status"
      className="flex size-full flex-col items-center justify-center p-(--space-8)"
    >
      <EmptyState
        variant="illustrated"
        title={SPACE_BOARD_EMPTY_TEXT}
        description="This board has no published work items yet."
        art={{ category: "general", type: "empty" }}
      />
      {onGoHome === undefined ? null : (
        <Button variant="secondary" size="small" onClick={onGoHome}>
          Back to all boards
        </Button>
      )}
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
  /** Called when the visitor follows the empty notice's action. */
  onGoHome?: (() => void) | undefined;
}

export function BoardLayouts({
  client,
  anchor,
  layout,
  filters,
  states,
  page,
  projectIdentifier,
  onGoHome,
}: BoardLayoutsProps): React.ReactElement {
  const { groups, paging, failed, loadMore } = useSpaceGroups(client, anchor, filters, states, page);
  if (page.total_count === 0) return <EmptyBoardNotice onGoHome={onGoHome} />;
  return layout === "kanban" ? (
    <SpaceBoardLayout
      groups={groups}
      paging={paging}
      failed={failed}
      onLoadMore={loadMore}
      projectIdentifier={projectIdentifier}
    />
  ) : (
    <SpaceListLayout
      groups={groups}
      paging={paging}
      failed={failed}
      onLoadMore={loadMore}
      projectIdentifier={projectIdentifier}
    />
  );
}
