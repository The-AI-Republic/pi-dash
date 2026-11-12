// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Applied-filter chip strip above the modules list (parity/specs/modules.md
// §2.7, MOD-007). One chip per active dimension, in spec order and with the
// stable keys the parity suite reads. Each value carries its own remove
// control; each chip carries one for the whole dimension; a trailing
// Clear all resets every dimension. The strip does not render at all while
// nothing is applied, and the name search never appears as a chip. The
// archived screen mounts the same strip with favorites and status hidden.

import { Avatar } from "@pidash/kit";
import type { WorkspaceMember } from "@pidash/api-client";
import { X } from "lucide-react";
import * as React from "react";

import { formatMergedRange } from "../../issues/index.js";
import {
  clearModuleFilterDimension,
  clearModuleFilters,
  removeModuleFilterValue,
  selectModuleChipGroups,
  type ModuleFilterDimension,
  type ModuleListFilters,
} from "../lib/moduleListControls.js";

export interface ModulesFilterChipsProps {
  filters: ModuleListFilters;
  members: readonly WorkspaceMember[] | undefined;
  meId: string | null;
  onFiltersChange: (filters: ModuleListFilters) => void;
  showFavorites?: boolean | undefined;
  showStatus?: boolean | undefined;
}

export function ModulesFilterChips({
  filters,
  members,
  meId,
  onFiltersChange,
  showFavorites = true,
  showStatus = true,
}: ModulesFilterChipsProps): React.ReactElement | null {
  const groups = selectModuleChipGroups(filters, {
    members,
    meId,
    showFavorites,
    showStatus,
    formatDateRange: formatMergedRange,
  });
  if (groups.length === 0) return null;

  return (
    <div data-testid="modules-filter-chips" className="flex flex-wrap items-center gap-(--space-2)">
      {groups.map((group) => (
        <div
          key={group.dimension}
          data-testid="modules-filter-chip"
          data-chip-key={group.key}
          data-chip-dimension={group.dimension}
          className="inline-flex flex-wrap items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) px-(--space-2) py-(--space-1)"
        >
          <span data-testid="modules-filter-chip-key" className="text-meta text-(--text-muted)">
            {group.key}
          </span>
          {group.values.map((chip) => (
            <span
              key={chip.value}
              data-testid="modules-filter-chip-value"
              data-chip-value={chip.value}
              className="text-meta inline-flex items-center gap-(--space-1) rounded-(--radius-full) bg-(--subtle) px-(--space-2) py-px"
            >
              {chip.color === undefined ? null : (
                <span
                  aria-hidden="true"
                  data-testid="modules-filter-chip-dot"
                  style={{ backgroundColor: chip.color }}
                  className="size-2 rounded-full"
                />
              )}
              {chip.avatarUrl === undefined || chip.avatarUrl === null ? null : (
                <Avatar name={chip.label} src={chip.avatarUrl} size="small" />
              )}
              <span>{chip.label}</span>
              <button
                type="button"
                aria-label={`Remove ${chip.label} from ${group.label}`}
                onClick={() => onFiltersChange(removeModuleFilterValue(filters, group.dimension, chip.value))}
                className="grid cursor-pointer place-items-center rounded-full text-(--text-muted) hover:text-(--text)"
              >
                <X size={12} strokeWidth={2} aria-hidden="true" />
              </button>
            </span>
          ))}
          <button
            type="button"
            aria-label={`Clear ${group.label} filter`}
            data-testid="modules-filter-chip-clear"
            onClick={() => onFiltersChange(clearModuleFilterDimension(filters, group.dimension))}
            className="grid cursor-pointer place-items-center rounded-full text-(--text-muted) hover:text-(--text)"
          >
            <X size={12} strokeWidth={2} aria-hidden="true" />
          </button>
        </div>
      ))}
      <button
        type="button"
        data-testid="modules-filter-clear-all"
        onClick={() => onFiltersChange(clearModuleFilters())}
        className="text-meta cursor-pointer rounded-(--radius-full) border border-(--border) px-(--space-3) py-px hover:bg-(--subtle)"
      >
        Clear all
      </button>
    </div>
  );
}

export type { ModuleFilterDimension };
