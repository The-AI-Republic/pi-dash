// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Applied-filter strip above the project views list (parity/specs/views.md
// §4, VIEW-007). Renders only while a filter dimension is active. Each
// dimension is one group: its label, one chip per selected value with its
// own remove control, and a remove control for the whole dimension. A
// trailing "Clear all" resets every dimension. Removing a chip edits only
// that value or dimension — sibling filters survive (the old app wiped
// them, bug NEWFRONT-216). The name search never renders a chip.

import { Avatar } from "@pidash/kit";
import type { WorkspaceMember } from "@pidash/api-client";
import { X } from "lucide-react";
import * as React from "react";

import {
  clearViewListFilters,
  removeViewFilterDimension,
  removeViewFilterValue,
  selectViewFilterChipGroups,
  type ViewFilterChipValue,
  type ViewFilterDimension,
  type ViewListFilters,
} from "../lib/viewListControls.js";

export interface ProjectViewsAppliedFiltersProps {
  filters: ViewListFilters;
  /** Roster for creator chip names and avatars. */
  members: readonly WorkspaceMember[] | undefined;
  selfId: string | null;
  onFiltersChange: (filters: ViewListFilters) => void;
}

function ValueChip({
  chip,
  dimension,
  onRemove,
}: {
  chip: ViewFilterChipValue;
  dimension: ViewFilterDimension;
  onRemove: (dimension: ViewFilterDimension, value: string) => void;
}): React.ReactElement {
  return (
    <span className="text-meta inline-flex items-center gap-(--space-1) rounded-(--radius-full) bg-(--subtle) px-(--space-2) py-px">
      {chip.avatarUrl === undefined ? null : <Avatar name={chip.label} src={chip.avatarUrl} size="small" />}
      <span>{chip.label}</span>
      <button
        type="button"
        aria-label={`Remove ${chip.label}`}
        data-chip-value={chip.value}
        onClick={() => onRemove(dimension, chip.value)}
        className="grid cursor-pointer place-items-center rounded-full text-(--text-muted) hover:text-(--text)"
      >
        <X size={12} strokeWidth={2} aria-hidden="true" />
      </button>
    </span>
  );
}

export function ProjectViewsAppliedFilters({
  filters,
  members,
  selfId,
  onFiltersChange,
}: ProjectViewsAppliedFiltersProps): React.ReactElement | null {
  const groups = selectViewFilterChipGroups(filters, members, selfId);
  if (groups.length === 0) return null;

  return (
    <div data-testid="project-views-applied-filters" className="flex flex-wrap items-center gap-(--space-2)">
      {groups.map((group) => (
        <span
          key={group.dimension}
          data-chip-dimension={group.dimension}
          className="inline-flex flex-wrap items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) px-(--space-2) py-(--space-1)"
        >
          <span className="text-meta text-(--text-muted)">{group.label}</span>
          {group.values.map((chip) => (
            <ValueChip
              key={chip.value}
              chip={chip}
              dimension={group.dimension}
              onRemove={(dimension, value) => onFiltersChange(removeViewFilterValue(filters, dimension, value))}
            />
          ))}
          <button
            type="button"
            aria-label={`Clear ${group.label} filters`}
            onClick={() => onFiltersChange(removeViewFilterDimension(filters, group.dimension))}
            className="grid cursor-pointer place-items-center rounded-full text-(--text-muted) hover:text-(--text)"
          >
            <X size={12} strokeWidth={2} aria-hidden="true" />
          </button>
        </span>
      ))}
      <button
        type="button"
        onClick={() => onFiltersChange(clearViewListFilters())}
        className="text-meta cursor-pointer rounded-(--radius-full) border border-(--border) px-(--space-3) py-px hover:bg-(--subtle)"
      >
        Clear all
      </button>
    </div>
  );
}
