// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Applied cycles filters (parity/specs/cycles.md §4, CYC-008). One group
// per dimension with a removable pill per applied value, a dimension
// clear, and one clear-all that resets every filter while the search
// text — with its own clear — keeps filtering. Reads the same store
// the menu writes, so chips, the trigger badge and the no-match view
// never disagree. Guests clear as freely as members (spec §20).

import { X } from "lucide-react";
import * as React from "react";

import { DATE_PRESETS } from "../../issues/index.js";
import { useCycleFilterStore } from "../api/filterStore.js";
import { lifecycleLabel } from "../lib/cycleGroups.js";
import {
  CYCLE_FILTER_CHIP_LABELS,
  hasCycleFilters,
  parseCycleDateEntry,
  type CycleFilterDimension,
} from "../lib/cycleFilters.js";

export interface CycleFilterChipsProps {
  projectId: string;
}

/** One date entry's pill text: the preset name or its bound. */
function dateChipLabel(entry: string): string {
  const parsed = parseCycleDateEntry(entry);
  if (parsed.kind === "preset") {
    return DATE_PRESETS.find((preset) => preset.value === entry)?.label ?? entry;
  }
  if (parsed.kind === "custom") {
    return `${parsed.direction === "after" ? "After" : "Before"} ${parsed.day}`;
  }
  return entry;
}

/** One applied value's pill text. */
function chipValueLabel(dimension: CycleFilterDimension, value: string): string {
  if (dimension === "status") return lifecycleLabel(value);
  return dateChipLabel(value);
}

const CHIP_DIMENSIONS: readonly CycleFilterDimension[] = ["status", "startDate", "endDate"];

export function CycleFilterChips({ projectId }: CycleFilterChipsProps): React.ReactElement | null {
  const filters = useCycleFilterStore((state) => state.filtersFor(projectId));
  if (!hasCycleFilters(filters)) return null;
  const dimensions = CHIP_DIMENSIONS.filter((dimension) => filters[dimension].length > 0);
  return (
    <div data-testid="cycles-filter-chips" className="flex flex-wrap items-center gap-(--space-2)">
      {dimensions.map((dimension) => {
        const label = CYCLE_FILTER_CHIP_LABELS[dimension];
        return (
          <span
            key={dimension}
            data-testid={`cycles-filter-chip-${dimension}`}
            className="inline-flex flex-wrap items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) px-(--space-2) py-(--space-1)"
          >
            <span className="text-meta text-(--text-muted)">{label}</span>
            {filters[dimension].map((value) => {
              const text = chipValueLabel(dimension, value);
              return (
                <span
                  key={value}
                  className="text-meta inline-flex items-center gap-(--space-1) rounded-(--radius-full) bg-(--subtle) px-(--space-2) py-px"
                >
                  {text}
                  <button
                    type="button"
                    aria-label={`Remove ${text}`}
                    onClick={() => useCycleFilterStore.getState().removeValue(projectId, dimension, value)}
                    className="grid cursor-pointer place-items-center rounded-full text-(--text-muted) hover:text-(--text)"
                  >
                    <X size={12} strokeWidth={2} aria-hidden="true" />
                  </button>
                </span>
              );
            })}
            <button
              type="button"
              aria-label={`Clear ${label} filters`}
              onClick={() => useCycleFilterStore.getState().clearDimension(projectId, dimension)}
              className="grid cursor-pointer place-items-center rounded-full text-(--text-muted) hover:text-(--text)"
            >
              <X size={12} strokeWidth={2} aria-hidden="true" />
            </button>
          </span>
        );
      })}
      <button
        type="button"
        onClick={() => useCycleFilterStore.getState().clearAll(projectId)}
        className="text-meta cursor-pointer rounded-(--radius-full) border border-(--border) px-(--space-3) py-px hover:bg-(--subtle)"
      >
        Clear all
      </button>
    </div>
  );
}
