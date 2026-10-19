// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Roster skeleton (parity/specs/workspace-settings.md §9, WSET-014). The
// section shows placeholder rows until the members and invitations reads
// resolve; the old app's loader could never pass its guard (bug
// NEWFRONT-422), and this is the intended behavior.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

export const ROSTER_SKELETON_ROWS = 6;

export function MembersSkeleton(): React.ReactElement {
  return (
    <div
      data-testid="members-skeleton"
      role="status"
      aria-label="Loading members"
      className="flex flex-col gap-(--space-2)"
    >
      <span className="sr-only">Loading members</span>
      {Array.from({ length: ROSTER_SKELETON_ROWS }, (_, index) => (
        <div key={index} className="flex items-center gap-(--space-4) py-(--space-2)" aria-hidden="true">
          <Skeleton width="24px" height="24px" round />
          <Skeleton width="160px" />
          <Skeleton width="120px" />
          <Skeleton width="100px" />
        </div>
      ))}
    </div>
  );
}
