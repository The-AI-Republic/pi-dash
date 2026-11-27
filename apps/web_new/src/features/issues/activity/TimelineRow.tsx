// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One row of the issue timeline: the 1px rail, the 28px type badge hung on
// it, and the entry content. Every row paints its own rail segment and the
// feed stacks rows with no gap, so the segments read as one line; the first
// and last rows drop their outer padding so the rail stops at the end
// badges instead of running past them.

import { cn } from "@pidash/kit";
import * as React from "react";

/** Where a row sits in the timeline, for trimming the rail. */
export type TimelinePosition = "only" | "first" | "middle" | "last";

/** Position of one row by index; a lone row trims both ends. */
export function timelinePosition(index: number, total: number): TimelinePosition {
  if (total <= 1) return "only";
  if (index <= 0) return "first";
  if (index >= total - 1) return "last";
  return "middle";
}

const ROW_PADDING: Record<TimelinePosition, string> = {
  only: "",
  first: "pb-(--space-2)",
  middle: "py-(--space-2)",
  last: "pt-(--space-2)",
};

export interface TimelineRowProps {
  position: TimelinePosition;
  /** Accessible name for the badge, such as the activity type. */
  badgeLabel: string;
  /** 14px glyph that names the entry type at a glance. */
  badge: React.ReactNode;
  children: React.ReactNode;
}

export function TimelineRow({ position, badgeLabel, badge, children }: TimelineRowProps): React.ReactElement {
  return (
    <div className={cn("relative flex gap-(--space-3)", ROW_PADDING[position])}>
      <div aria-hidden="true" className="absolute top-0 bottom-0 left-[13px] w-px bg-(--border)" />
      <div
        role="img"
        aria-label={badgeLabel}
        className="relative z-[1] flex size-7 shrink-0 items-center justify-center rounded-(--radius-card) border border-(--border) bg-(--subtle) text-(--text-muted)"
      >
        {badge}
      </div>
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}
