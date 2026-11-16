// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle completion ring (parity/specs/cycles.md §2, CYC-002). The arc
// fills to the cycle's percent with the number in the middle; an
// exactly complete cycle shows a check instead of the number, and a
// zero denominator reads 0% on an empty ring, never a full one.

import { Check } from "lucide-react";
import * as React from "react";

export interface CycleProgressRingProps {
  /** Completion percent; clamped into 0..100 for the arc. */
  percent: number;
}

const RING_SIZE = 30;
const RING_STROKE = 3;
const RING_RADIUS = (RING_SIZE - RING_STROKE) / 2;
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;

export function CycleProgressRing({ percent }: CycleProgressRingProps): React.ReactElement {
  const clamped = Math.min(100, Math.max(0, percent));
  const complete = clamped === 100;
  return (
    <span
      data-testid="cycle-progress-ring"
      role="img"
      aria-label={`${clamped}% complete`}
      className="relative inline-flex shrink-0 items-center justify-center"
      style={{ width: RING_SIZE, height: RING_SIZE }}
    >
      <svg aria-hidden="true" width={RING_SIZE} height={RING_SIZE} className="absolute inset-0">
        <circle
          cx={RING_SIZE / 2}
          cy={RING_SIZE / 2}
          r={RING_RADIUS}
          fill="none"
          stroke="var(--border)"
          strokeWidth={RING_STROKE}
        />
        <circle
          cx={RING_SIZE / 2}
          cy={RING_SIZE / 2}
          r={RING_RADIUS}
          fill="none"
          stroke="var(--accent)"
          strokeWidth={RING_STROKE}
          strokeLinecap="round"
          strokeDasharray={`${(RING_CIRCUMFERENCE * clamped) / 100} ${RING_CIRCUMFERENCE}`}
          transform={`rotate(-90 ${RING_SIZE / 2} ${RING_SIZE / 2})`}
        />
      </svg>
      {complete ? (
        <>
          <Check size={14} strokeWidth={2.5} aria-hidden="true" className="text-(--accent)" />
          <span className="sr-only">100%</span>
        </>
      ) : (
        <span className="text-caption text-(--text)">{clamped}%</span>
      )}
    </span>
  );
}
