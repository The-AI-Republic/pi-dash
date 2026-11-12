// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overdue badge for the due-date row (ISS-236). Cloud-only: the community
// build renders nothing here, so the row carries only its date. The badge
// reads the same overdue predicate the row already computes, so the two
// never disagree about what is late.

import * as React from "react";

import { overdueBadgeLabel, showsDetailAccents } from "./badges.js";

export interface OverdueBadgeProps {
  /** The stored due day, YYYY-MM-DD. */
  dueDay: string;
  /** Today's calendar day in the same shape. */
  today: string;
  /** Whether the item is still open; closed rows never show the badge. */
  overdue: boolean;
}

export function OverdueBadge({ dueDay, today, overdue }: OverdueBadgeProps): React.ReactElement | null {
  if (!showsDetailAccents() || !overdue) return null;
  const label = overdueBadgeLabel(dueDay, today);
  if (label === "") return null;
  return (
    <span
      data-testid="detail-overdue-badge"
      className="text-meta shrink-0 rounded-(--radius-control) bg-(--danger)/10 px-(--space-2) py-px font-medium text-(--danger)"
    >
      {label}
    </span>
  );
}
