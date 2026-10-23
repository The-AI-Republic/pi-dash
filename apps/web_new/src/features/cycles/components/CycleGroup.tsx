// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One collapsible cycle group (parity/specs/cycles.md §2, CYC-001).
// The heading is a disclosure button carrying the singular group name
// with its trailing count and the group's expanded state; collapsing
// hides the rows and keeps the count visible. Empty groups still
// render their header with a zero count.

import { ChevronDown } from "lucide-react";
import * as React from "react";

import { CYCLE_GROUP_HEADINGS, type CycleListGroup } from "../lib/cycleGroups.js";

export interface CycleGroupProps {
  group: CycleListGroup;
  count: number;
  expanded: boolean;
  onToggle: () => void;
  children?: React.ReactNode;
}

export function CycleGroup({ group, count, expanded, onToggle, children }: CycleGroupProps): React.ReactElement {
  return (
    <section data-testid={`cycle-group-${group}`} aria-label={CYCLE_GROUP_HEADINGS[group]} className="flex flex-col">
      <button
        type="button"
        aria-expanded={expanded}
        onClick={onToggle}
        className="flex cursor-pointer items-center gap-(--space-2) px-(--space-4) py-(--space-2) text-left"
      >
        <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" className={expanded ? undefined : "-rotate-90"} />
        <span className="text-emphasis text-(--text)">{CYCLE_GROUP_HEADINGS[group]}</span>
        <span data-testid="cycle-group-count" className="text-caption text-(--text-muted)">
          {count}
        </span>
      </button>
      {expanded ? children : null}
    </section>
  );
}
