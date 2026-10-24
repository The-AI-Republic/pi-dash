// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// SPC-024/026: the sideways board. One fixed-width column per workflow state
// in board order, laid out in a horizontally scrolling strip; each column
// scrolls its own cards vertically under a sticky header and keeps its empty
// shape when it has none. A column asks for its next page when it is bottomed
// out — either its tripwire comes into view or a scroll gesture ends there —
// once per gesture, thanks to the debounce, and placeholder cards hold the
// place while the page is in flight.

import * as React from "react";

import type { SpaceGroupsState } from "./useSpaceGroups.js";
import { groupHasNextPage, type SpaceIssueGroup } from "./model.js";
import { debounce } from "./debounce.js";
import { GroupHeader } from "./GroupHeader.js";
import { SpaceIssueRow } from "./SpaceIssueRow.js";
import { useLoadMoreOnIntersect } from "./useLoadMoreOnIntersect.js";

/** Quiet window a scroll burst must pass before the next page is requested. */
export const SPACE_BOARD_SCROLL_DEBOUNCE_MS = 250;

/** How close to the column's end a scroll gesture counts as reaching it. */
export const SPACE_BOARD_END_REACH_PX = 48;

export interface SpaceBoardLayoutProps {
  groups: readonly SpaceIssueGroup[];
  paging: ReadonlySet<string>;
  failed: ReadonlySet<string>;
  onLoadMore: SpaceGroupsState["loadMore"];
  projectIdentifier: string;
}

export function SpaceBoardLayout({
  groups,
  paging,
  failed,
  onLoadMore,
  projectIdentifier,
}: SpaceBoardLayoutProps): React.ReactElement {
  return (
    <div
      data-testid="space-board-layout"
      className="flex size-full gap-(--space-2) overflow-x-auto overflow-y-hidden p-(--space-4)"
    >
      {groups.map((group) => (
        <BoardColumn
          key={group.stateId}
          group={group}
          paging={paging.has(group.stateId)}
          failed={failed.has(group.stateId)}
          onLoadMore={() => onLoadMore(group.stateId)}
          projectIdentifier={projectIdentifier}
        />
      ))}
    </div>
  );
}

interface BoardColumnProps {
  group: SpaceIssueGroup;
  paging: boolean;
  failed: boolean;
  onLoadMore: () => void;
  projectIdentifier: string;
}

function BoardColumn({ group, paging, failed, onLoadMore, projectIdentifier }: BoardColumnProps): React.ReactElement {
  // The observer root is state, not a ref value: the scroller mounts in this
  // same render, so a ref read during render would still be null.
  const [scroller, setScroller] = React.useState<HTMLDivElement | null>(null);
  const hasNext = groupHasNextPage(group);
  // One debounced trigger per column for its whole life: the render that a
  // page request causes must not cancel the call that started it. The
  // newest callback is read through a ref instead.
  const latest = React.useRef(onLoadMore);
  latest.current = onLoadMore;
  const request = React.useMemo(() => debounce(() => latest.current(), SPACE_BOARD_SCROLL_DEBOUNCE_MS), []);
  React.useEffect(() => () => request.cancel(), [request]);

  const sentinel = useLoadMoreOnIntersect({
    root: scroller,
    // A column watches its own scroller, so it waits for one: observing the
    // page instead would page whichever column happened to be near the fold.
    // After a failed page the tripwire stays disarmed while the sentinel is
    // still in view; the visitor's next scroll gesture to the end retries,
    // so a broken request cannot spin by itself.
    enabled: hasNext && !paging && !failed && scroller !== null,
    onLoadMore: request,
  });

  return (
    <div
      data-testid="space-board-column"
      data-state-id={group.stateId}
      data-failed={failed ? "true" : undefined}
      className="flex w-[300px] shrink-0 flex-col rounded-(--radius-control) bg-(--subtle)"
    >
      <div data-testid="space-board-column-header" className="sticky top-0 z-1 shrink-0 bg-(--subtle) p-(--space-1)">
        <GroupHeader state={group.state} count={group.total} />
      </div>
      <div
        ref={setScroller}
        data-testid="space-board-column-scroll"
        onScroll={(event) => {
          // Only a gesture that reaches the column's end asks for more, so
          // scrolling back up (or around a short column that is not paged
          // out) never fetches; the debounce folds the gesture into one
          // request.
          if (!hasNext || paging) return;
          if (!isAtEnd(event.currentTarget)) return;
          request();
        }}
        className="flex min-h-0 flex-1 flex-col gap-(--space-1) overflow-y-auto p-(--space-1)"
      >
        {group.issues.map((issue) => (
          <SpaceIssueRow key={issue.id} issue={issue} projectIdentifier={projectIdentifier} card />
        ))}
        {paging ? <PlaceholderCards /> : null}
        <div ref={sentinel} data-testid="space-board-column-sentinel" className="h-px w-full shrink-0" />
      </div>
    </div>
  );
}

/** Whether the scroller is at (or within reach of) its bottom edge. */
function isAtEnd(node: HTMLElement): boolean {
  return node.scrollTop + node.clientHeight >= node.scrollHeight - SPACE_BOARD_END_REACH_PX;
}

/** Two pulsing cards that hold the column while its next page loads. */
export function PlaceholderCards(): React.ReactElement {
  return (
    <div data-testid="space-board-card-placeholder" aria-hidden="true" className="flex flex-col gap-(--space-1)">
      {[0, 1].map((index) => (
        <span key={index} className="block h-20 animate-pulse rounded-(--radius-control) bg-(--border)" />
      ))}
    </div>
  );
}
