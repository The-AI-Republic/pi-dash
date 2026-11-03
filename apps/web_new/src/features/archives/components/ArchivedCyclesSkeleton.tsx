// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-cycles loading skeleton (parity/specs/archives.md §4.4,
// ARCH-017, ARCH-030). Row shapes only: the header controls render
// immediately, so the placeholder covers just the list region. It is
// mutually exclusive with rows and every settled state — the screen
// swaps it out in one commit once the read settles, and a background
// refetch never brings it back.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

const PLACEHOLDER_ROWS = 4;

export function ArchivedCyclesSkeleton(): React.ReactElement {
  return (
    <div data-testid="archived-cycles-skeleton" className="flex flex-col">
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
  );
}
