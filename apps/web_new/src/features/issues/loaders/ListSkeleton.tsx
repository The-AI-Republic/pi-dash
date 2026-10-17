// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// List loading placeholder (parity/specs/issues.md §1, ISS-004/ISS-005).
// EAGER LEAF: the route pending component renders this while the prefetch
// runs, so it stands alone — the remaining per-layout skeletons stay in
// the lazy skeletons module (NEWFRONT-287).

import { Skeleton } from "@pidash/kit";
import * as React from "react";

const PLACEHOLDER_ROWS = 8;

function rows(count: number): number[] {
  return Array.from({ length: count }, (_, index) => index);
}

/** List placeholder: key, title and badge shapes per row. */
export function ListSkeleton(): React.ReactElement {
  return (
    <div data-testid="issues-skeleton-list" className="flex h-full flex-col">
      {rows(PLACEHOLDER_ROWS).map((index) => (
        <div
          key={index}
          data-skeleton-row="true"
          className="flex h-11 items-center gap-(--space-3) border-b border-(--border) px-(--space-4)"
        >
          <Skeleton width="64px" />
          <Skeleton height="14px" />
          <Skeleton width="48px" />
        </div>
      ))}
    </div>
  );
}
