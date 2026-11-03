// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The priority marker shared by the filter panel and the applied pills:
// the shared level glyph in its tone, never the sole carrier (the level
// name always sits beside it).

import * as React from "react";

import { PRIORITY_ICONS, PRIORITY_TONES, type PriorityLevel } from "../../../shared/pickers/priorities.js";

import type { SpacePriorityLevel } from "./facets.js";

export interface PriorityMarkerProps {
  level: SpacePriorityLevel;
  size?: number;
}

export function PriorityMarker({ level, size = 12 }: PriorityMarkerProps): React.ReactElement {
  const key = level as PriorityLevel;
  const Glyph = PRIORITY_ICONS[key];
  return (
    <span
      data-testid="space-priority-marker"
      data-level={level}
      aria-hidden="true"
      className="inline-flex shrink-0 items-center justify-center"
    >
      <Glyph size={size} strokeWidth={2} className={PRIORITY_TONES[key]} aria-hidden="true" />
    </span>
  );
}
