// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail loading skeleton (parity/specs/issues.md §13, ISS-142/144).
// The browse route shows this while the two-phase load resolves: main
// column bars plus the sidebar bars the metadata hydrate fills in.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

export function DetailSkeleton(): React.ReactElement {
  return (
    <div
      role="status"
      aria-label="Loading work item"
      className="flex flex-col gap-(--space-8) p-(--space-8) md:flex-row"
    >
      <div className="flex min-w-0 flex-1 flex-col gap-(--space-4)">
        <Skeleton height="30px" width="40%" />
        <Skeleton height="15px" width="60%" />
        <Skeleton height="15px" width="60%" />
        <Skeleton height="15px" width="40%" />
      </div>
      <div className="flex min-w-0 flex-col gap-(--space-4) md:w-80 md:shrink-0">
        <Skeleton height="30px" />
        <Skeleton height="30px" />
        <Skeleton height="30px" />
        <Skeleton height="30px" />
      </div>
    </div>
  );
}
