// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loading placeholder sized to the active layout (parity/specs/modules.md
// §3.1, MOD-008): one row per module for the list, a gallery of cards for
// the board, and name bars beside a chart band for the timeline. Exclusive
// by construction: the screen renders this in place of content, so it
// never shares a commit with rows, cards, bars or an empty state, and a
// background refetch never brings it back.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

import type { ModulesLayout } from "../lib/moduleLayouts.js";

/** Placeholder shapes per layout, enough to fill the first screen. */
const PLACEHOLDER_COUNT = 6;

export interface ModulesListSkeletonProps {
  layout: ModulesLayout;
}

function ListSkeleton(): React.ReactElement {
  return (
    <div data-skeleton-list="true" className="flex flex-col">
      {Array.from({ length: PLACEHOLDER_COUNT }, (_, index) => (
        <div
          key={index}
          data-skeleton-row="true"
          className="flex h-14 items-center gap-(--space-3) border-b border-(--border) px-(--space-4)"
        >
          <Skeleton width="24px" height="24px" round />
          <Skeleton height="14px" />
          <Skeleton width="120px" />
          <Skeleton width="96px" height="20px" />
        </div>
      ))}
    </div>
  );
}

function BoardSkeleton(): React.ReactElement {
  return (
    <div data-skeleton-board="true" className="grid grid-cols-1 gap-(--space-4) md:grid-cols-2 xl:grid-cols-3">
      {Array.from({ length: PLACEHOLDER_COUNT }, (_, index) => (
        <div
          key={index}
          data-skeleton-card="true"
          className="flex flex-col gap-(--space-3) rounded-(--radius-control) border border-(--border) p-(--space-4)"
        >
          <div className="flex items-center justify-between gap-(--space-3)">
            <Skeleton width="60%" height="16px" />
            <Skeleton width="28px" height="28px" round />
          </div>
          <Skeleton height="8px" />
          <div className="flex items-center gap-(--space-2)">
            <Skeleton width="20px" height="20px" round />
            <Skeleton width="72px" />
          </div>
        </div>
      ))}
    </div>
  );
}

function TimelineSkeleton(): React.ReactElement {
  return (
    <div data-skeleton-timeline="true" className="flex flex-col">
      {Array.from({ length: PLACEHOLDER_COUNT }, (_, index) => (
        <div
          key={index}
          data-skeleton-bar-row="true"
          className="flex h-12 items-center gap-(--space-3) border-b border-(--border) px-(--space-4)"
        >
          <Skeleton width="160px" height="14px" />
          <Skeleton height="10px" className="min-w-0 flex-1" />
        </div>
      ))}
    </div>
  );
}

export function ModulesListSkeleton({ layout }: ModulesListSkeletonProps): React.ReactElement {
  return (
    <div
      data-testid="modules-list-skeleton"
      data-layout={layout}
      role="status"
      aria-live="polite"
      className="flex min-h-0 flex-1 flex-col"
    >
      <span className="sr-only">Loading modules</span>
      {layout === "board" ? <BoardSkeleton /> : layout === "timeline" ? <TimelineSkeleton /> : <ListSkeleton />}
    </div>
  );
}
