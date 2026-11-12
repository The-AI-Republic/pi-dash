// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The board card's segmented progress bar (parity/specs/modules.md §3.6,
// MOD-020). One slice per non-empty work-item state group, each drawn at a
// width proportional to its share of the module's work items and tinted
// with that group's hue. An empty module keeps the bar's track but draws no
// slices, so an empty card never looks like a full one.
//
// The slices are decorative: the bar's accessible name sums the mix up in
// words, which is what a reader should hear rather than five anonymous
// percentages.

import type { ModuleRow } from "@pidash/api-client";
import * as React from "react";

import { moduleProgressSegments, type ModuleProgressSegment } from "../lib/moduleIdentity.js";

export interface ModuleSegmentedBarProps {
  module: Pick<
    ModuleRow,
    "total_issues" | "completed_issues" | "started_issues" | "unstarted_issues" | "backlog_issues" | "cancelled_issues"
  >;
}

/** The words a reader hears for one segment's mix. */
export function moduleSegmentSummary(segments: readonly ModuleProgressSegment[]): string {
  if (segments.length === 0) return "No work items yet";
  return segments.map((segment) => `${segment.count} ${segment.label.toLowerCase()}`).join(", ");
}

export function ModuleSegmentedBar({ module }: ModuleSegmentedBarProps): React.ReactElement {
  const segments = moduleProgressSegments(module);
  return (
    <div
      role="img"
      aria-label={`Work items by state: ${moduleSegmentSummary(segments)}`}
      data-testid="modules-card-progress"
      className="flex h-1.5 w-full gap-0.5 overflow-hidden rounded-full bg-(--subtle)"
    >
      {segments.map((segment) => (
        <div
          key={segment.group}
          data-testid="modules-card-progress-segment"
          data-segment-group={segment.group}
          data-segment-count={segment.count}
          style={{ width: `${segment.width}%`, backgroundColor: segment.color }}
          className="h-full"
        />
      ))}
    </div>
  );
}
