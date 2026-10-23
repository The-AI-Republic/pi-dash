// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cursor-paging tail of the intake list (parity/specs/intake.md INT-007).
// While another page exists, placeholder rows render below the loaded
// rows with the paging sentinel under them; the sentinel sits empty
// once the list is exhausted. The scroll pane owns the scroll listener
// and reads `nearListEnd`; this component owns the placeholders and the
// sentinel they stand over.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

/** Placeholder rows per pending page; enough to fill the pane's tail. */
export const INTAKE_PAGE_SKELETON_COUNT = 3;

export interface IntakeListPagerProps {
  /** Whether the server reports another page. */
  hasNextPage: boolean;
}

export function IntakeListPager({ hasNextPage }: IntakeListPagerProps): React.ReactElement {
  if (!hasNextPage) return <div data-testid="intake-pager-sentinel" aria-hidden="true" />;
  return (
    <>
      <div data-testid="intake-page-skeletons" aria-hidden="true" className="flex flex-col">
        {Array.from({ length: INTAKE_PAGE_SKELETON_COUNT }, (_, index) => (
          <div
            key={index}
            data-testid="intake-page-skeleton"
            className="flex h-11 items-center gap-(--space-3) border-b border-(--border) px-(--space-4)"
          >
            <Skeleton height="14px" />
          </div>
        ))}
      </div>
      <div data-testid="intake-pager-sentinel" aria-hidden="true" />
    </>
  );
}
