// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// List loading skeleton (parity/specs/cycles.md §5, CYC-012): the
// header, the hero slot and a few row shapes. Mutually exclusive with
// rows and every empty state: the screen swaps it out in one commit
// once data arrives, and a background refetch never brings it back.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

const PLACEHOLDER_ROWS = 4;

export function CyclesListSkeleton(): React.ReactElement {
  return (
    <div data-testid="cycles-list-skeleton" className="flex flex-col gap-(--space-4)">
      <div className="flex items-center justify-between gap-(--space-3)">
        <Skeleton width="180px" height="20px" />
        <div className="flex items-center gap-(--space-2)">
          <Skeleton width="120px" height="28px" />
          <Skeleton width="96px" height="28px" />
        </div>
      </div>
      <div
        data-skeleton-hero="true"
        className="flex flex-col gap-(--space-3) rounded-(--radius-control) border border-(--border) p-(--space-4)"
      >
        <Skeleton width="40%" height="16px" />
        <Skeleton height="12px" />
        <div className="flex items-center gap-(--space-3)">
          <Skeleton width="48px" height="48px" round />
          <Skeleton height="12px" />
        </div>
      </div>
      <div className="flex flex-col">
        {Array.from({ length: PLACEHOLDER_ROWS }, (_, index) => (
          <div
            key={index}
            data-skeleton-row="true"
            className="flex h-14 items-center gap-(--space-3) border-b border-(--border) px-(--space-4)"
          >
            <Skeleton width="24px" height="24px" round />
            <Skeleton height="14px" />
            <Skeleton width="140px" />
            <Skeleton width="72px" height="20px" />
          </div>
        ))}
      </div>
    </div>
  );
}
