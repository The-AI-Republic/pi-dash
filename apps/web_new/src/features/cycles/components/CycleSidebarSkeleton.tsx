// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared cycle sidebar skeleton (parity/specs/cycles.md §14, CYC-040).
// One placeholder row behind both surfaces: the detail sidebar stands
// it in until the cycle and its progress data arrive, and the
// quick-look panel (slice B's shell) shows the same bars until the
// detail read lands. The minimum height keeps the layout from jumping
// when the content replaces it.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

export interface CycleSidebarSkeletonProps {
  /** Surface hook: the sidebar and the peek read their own test id. */
  testId: string;
}

export function CycleSidebarSkeleton({ testId }: CycleSidebarSkeletonProps): React.ReactElement {
  return (
    <div
      data-testid={testId}
      role="status"
      aria-label="Loading cycle details"
      className="flex min-h-64 flex-col gap-(--space-3)"
    >
      <Skeleton height="14px" width="45%" />
      <Skeleton height="12px" width="75%" />
      <Skeleton height="12px" width="55%" />
      <Skeleton height="12px" width="30%" />
    </div>
  );
}
