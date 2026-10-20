// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Group glyph with the row's share ring (parity/specs/
// project-settings-states.md, PSET-028). The icon names the workflow
// group; the ring around it fills to the row's share of its group, so a
// glance down a card reads the order at which a state sits.

import * as React from "react";

import { STATE_GROUP_ICONS } from "../../../shared/pickers/states.js";
import type { WorkflowGroup } from "../groups.js";

export interface GroupGlyphProps {
  group: WorkflowGroup;
  color: string;
  /** Share in 0..1; 0 draws the ring empty. */
  share: number;
  size?: number;
}

export function GroupGlyph({ group, color, share, size = 16 }: GroupGlyphProps): React.ReactElement {
  const Icon = STATE_GROUP_ICONS[group];
  const clamped = Math.min(1, Math.max(0, share));
  const track = size + 4;
  const radius = (track - 2) / 2;
  const circumference = 2 * Math.PI * radius;
  return (
    <span
      data-testid="state-group-glyph"
      data-share={clamped.toFixed(2)}
      className="relative inline-flex shrink-0 items-center justify-center"
      style={{ width: track, height: track }}
    >
      <svg aria-hidden="true" width={track} height={track} className="absolute inset-0">
        <circle cx={track / 2} cy={track / 2} r={radius} fill="none" stroke="var(--border)" strokeWidth={1.5} />
        <circle
          cx={track / 2}
          cy={track / 2}
          r={radius}
          fill="none"
          stroke={color}
          strokeWidth={1.5}
          strokeDasharray={`${circumference * clamped} ${circumference}`}
          transform={`rotate(-90 ${track / 2} ${track / 2})`}
        />
      </svg>
      <Icon size={size} strokeWidth={1.5} style={{ color }} aria-hidden="true" />
    </span>
  );
}
