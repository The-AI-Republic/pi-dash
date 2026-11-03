// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members skeleton (parity/specs/project-settings-members.md §"Loading and
// failure states"; PSET-027). Stands in until the membership and directory
// reads settle; it carries a status role so assistive tech hears the wait.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

/** Placeholder rows while the roster is unfetched. */
export const MEMBER_SKELETON_ROWS = 3;

export function MemberSkeleton(): React.ReactElement {
  return (
    <div
      data-testid="members-skeleton"
      role="status"
      aria-label="Loading members"
      className="rounded-(--radius-control) border border-(--border) px-(--space-4) py-(--space-2)"
    >
      <span className="sr-only">Loading members</span>
      <div aria-hidden="true" className="flex flex-col gap-(--space-3) py-(--space-2)">
        {Array.from({ length: MEMBER_SKELETON_ROWS }, (_, index) => (
          <span key={index} className="flex items-center gap-(--space-3)">
            <Skeleton width="24px" height="24px" round />
            <Skeleton width={`${60 + (index % 3) * 10}%`} />
          </span>
        ))}
      </div>
    </div>
  );
}
