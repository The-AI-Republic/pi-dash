// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// A small circular completion marker: a track ring with an accent arc that
// fills to the percent, and a caller-chosen glyph or short label in the
// middle. The arc is decorative; the ring's accessible name carries the
// value, so a screen reader hears a percentage even when the middle shows
// an icon.
import * as React from "react";
import { cn } from "../lib/cn";

export type ProgressRingSize = "small" | "medium";

export interface ProgressRingProps {
  /** Completion percent; clamped into 0..100 for the arc and the name. */
  percent: number;
  /** What sits inside the ring; the default is the percentage number. */
  children?: React.ReactNode;
  size?: ProgressRingSize;
  /** Accessible description when the caller wants more than "N% complete". */
  label?: string | undefined;
  className?: string | undefined;
}

const RING_PX: Record<ProgressRingSize, number> = { small: 24, medium: 30 };
const RING_STROKE_PX = 3;

function clampPercent(percent: number): number {
  if (!Number.isFinite(percent)) return 0;
  return Math.min(100, Math.max(0, Math.round(percent)));
}

export function ProgressRing({
  percent,
  children,
  size = "medium",
  label,
  className,
}: ProgressRingProps): React.ReactElement {
  const value = clampPercent(percent);
  const px = RING_PX[size];
  const radius = (px - RING_STROKE_PX) / 2;
  const circumference = 2 * Math.PI * radius;
  return (
    <span
      role="img"
      aria-label={label ?? `${value}% complete`}
      className={cn("relative inline-flex shrink-0 items-center justify-center", className)}
      style={{ width: px, height: px }}
    >
      <svg aria-hidden="true" width={px} height={px} className="absolute inset-0">
        <circle cx={px / 2} cy={px / 2} r={radius} fill="none" stroke="var(--border)" strokeWidth={RING_STROKE_PX} />
        <circle
          cx={px / 2}
          cy={px / 2}
          r={radius}
          fill="none"
          stroke="var(--accent)"
          strokeWidth={RING_STROKE_PX}
          strokeLinecap="round"
          strokeDasharray={`${(circumference * value) / 100} ${circumference}`}
          transform={`rotate(-90 ${px / 2} ${px / 2})`}
        />
      </svg>
      <span aria-hidden="true" className="relative inline-flex items-center justify-center">
        {children ?? <span className="text-caption text-(--text-muted)">{value}%</span>}
      </span>
    </span>
  );
}
