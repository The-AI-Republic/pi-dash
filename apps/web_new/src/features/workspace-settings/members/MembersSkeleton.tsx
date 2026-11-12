// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Roster skeleton (parity/specs/workspace-settings.md §9, WSET-014). The
// section shows a quiet stack of placeholder bars until the members and
// invitations reads resolve; the old app's loader could never pass its
// guard (bug NEWFRONT-422), so this is the intended behavior.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

/** Placeholder lines, each a share of the card's width. */
export const ROSTER_SKELETON_LINES = [92, 71, 84, 63] as const;

export function MembersSkeleton(): React.ReactElement {
  return (
    <div
      data-testid="members-skeleton"
      role="status"
      aria-label="Loading members"
      className="rounded-(--radius-control) border border-(--border) px-(--space-4) py-(--space-2)"
    >
      <span className="sr-only">Loading members</span>
      <div aria-hidden="true" className="flex flex-col gap-(--space-3) py-(--space-2)">
        {ROSTER_SKELETON_LINES.map((percent) => (
          <Skeleton key={percent} width={`${percent}%`} />
        ))}
      </div>
    </div>
  );
}
