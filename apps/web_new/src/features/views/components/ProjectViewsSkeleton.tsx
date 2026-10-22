// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Loading skeleton (parity/specs/views.md §3, VIEW-008). Row placeholders
// sized like the real rows, inside the same content frame. Mutually
// exclusive with rows and both empty states: the screen swaps it out in
// one commit once data arrives.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

const PLACEHOLDER_ROWS = 6;

export function ProjectViewsSkeleton(): React.ReactElement {
  return (
    <div data-testid="project-views-skeleton" className="flex flex-col">
      {Array.from({ length: PLACEHOLDER_ROWS }, (_, index) => (
        <div
          key={index}
          data-skeleton-row="true"
          className="flex h-11 items-center gap-(--space-3) border-b border-(--border) px-(--space-4)"
        >
          <Skeleton width="16px" height="16px" />
          <Skeleton height="14px" />
          <Skeleton width="64px" />
          <Skeleton width="24px" height="24px" round />
        </div>
      ))}
    </div>
  );
}
