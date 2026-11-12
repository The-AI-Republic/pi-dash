// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Group glyph with an optional share ring (parity/specs/
// project-settings-states.md, PSET-028). The icon names the workflow
// group. On a state row the ring around it fills to that state's share
// of its group, so a glance down a card reads where a state sits; a card
// header draws the icon alone, since a group has no share of itself.

import * as React from "react";

import { STATE_GROUP_ICONS } from "../../../shared/pickers/states.js";
import type { WorkflowGroup } from "../groups.js";

export interface GroupGlyphProps {
  group: WorkflowGroup;
  color: string;
  /** Share in 0..1; 0 draws the ring empty, absent draws no ring at all. */
  share?: number;
  size?: number;
}

export function GroupGlyph({ group, color, share, size = 16 }: GroupGlyphProps): React.ReactElement {
  const Icon = STATE_GROUP_ICONS[group];
  const clamped = share === undefined ? null : Math.min(1, Math.max(0, share));
  const track = size + 4;
  const radius = (track - 2) / 2;
  const circumference = 2 * Math.PI * radius;
  return (
    <span
      data-testid="state-group-glyph"
      data-share={clamped === null ? undefined : clamped.toFixed(2)}
      className="relative inline-flex shrink-0 items-center justify-center"
      style={{ width: track, height: track }}
    >
      {clamped === null ? null : (
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
      )}
      <Icon size={size} strokeWidth={1.5} style={{ color }} aria-hidden="true" />
    </span>
  );
}
