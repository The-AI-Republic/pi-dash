// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-modules loading placeholder (parity/specs/modules.md §7.3,
// MOD-027). The header controls render immediately; only the row region is
// covered. It is exclusive with every settled state, and a background
// refetch never brings it back.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

const PLACEHOLDER_ROWS = 4;

export function ArchivedModulesSkeleton(): React.ReactElement {
  return (
    <div
      data-testid="archived-modules-skeleton"
      role="status"
      aria-label="Loading archived modules"
      className="flex flex-col"
    >
      {Array.from({ length: PLACEHOLDER_ROWS }, (_, index) => (
        <div
          key={index}
          data-skeleton-row="true"
          className="flex h-14 items-center gap-(--space-3) border-b border-(--border) px-(--space-4)"
        >
          <Skeleton width="24px" height="24px" round />
          <Skeleton height="14px" />
          <Skeleton width="140px" />
          <Skeleton width="96px" height="20px" />
        </div>
      ))}
    </div>
  );
}
