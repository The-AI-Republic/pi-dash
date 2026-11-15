// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail loading skeleton (parity/specs/issues.md §13, ISS-142/144).
// The browse route shows this while the two-phase load resolves: main
// column bars plus the sidebar bars the metadata hydrate fills in.
// Narrow stacking matches the live layout (NEWPIDASH-233): the sidebar
// bars sit between the main column and the activity placeholder.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

import { useNarrowViewport } from "../../../shared/shell/narrowViewport.js";

export function DetailSkeleton(): React.ReactElement {
  const narrow = useNarrowViewport();
  const gridStyle: React.CSSProperties = narrow
    ? {
        gridTemplateAreas: '"main" "sidebar" "activity"',
        gridTemplateColumns: "minmax(0,1fr)",
        gap: "var(--space-8)",
      }
    : {
        gridTemplateAreas: '"main sidebar" "activity sidebar"',
        gridTemplateColumns: "minmax(0,1fr) 20rem",
        rowGap: "var(--space-6)",
        columnGap: "var(--space-8)",
      };
  return (
    <div role="status" aria-label="Loading work item" className="grid gap-(--space-8) p-(--space-8)" style={gridStyle}>
      <div className="flex min-w-0 flex-col gap-(--space-4)" style={{ gridArea: "main" }}>
        <Skeleton height="30px" width="40%" />
        <Skeleton height="15px" width="60%" />
        <Skeleton height="15px" width="60%" />
        <Skeleton height="15px" width="40%" />
      </div>
      <div className="flex min-w-0 flex-col gap-(--space-4)" style={{ gridArea: "sidebar" }}>
        <Skeleton height="30px" />
        <Skeleton height="30px" />
        <Skeleton height="30px" />
        <Skeleton height="30px" />
      </div>
      <div className="flex min-w-0 flex-col gap-(--space-4)" style={{ gridArea: "activity" }}>
        <Skeleton height="30px" width="80%" />
        <Skeleton height="15px" width="100%" />
        <Skeleton height="15px" width="90%" />
      </div>
    </div>
  );
}
