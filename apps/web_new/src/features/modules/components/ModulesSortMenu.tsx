// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Ordering menu for the modules list (parity/specs/modules.md §2.5,
// MOD-005). The trigger names the active key; the menu lists the six keys,
// then a divider and the two directions — except under Manual placement,
// which has no direction and shows the keys alone. Re-picking the current
// key or direction changes nothing; picking Manual leaves the stored order
// as-is.

import { Button, Menu } from "@pidash/kit";
import { ArrowDownWideNarrow, Check } from "lucide-react";
import * as React from "react";

import {
  MODULE_SORT_DIRECTION_LABELS,
  MODULE_SORT_DIRECTIONS,
  MODULE_SORT_LABELS,
  MODULE_SORT_OPTIONS,
  moduleSortHasDirection,
  selectModuleSort,
  type ModuleSort,
} from "../lib/moduleListControls.js";

export interface ModulesSortMenuProps {
  sort: ModuleSort;
  onSortChange: (sort: ModuleSort) => void;
}

export function ModulesSortMenu({ sort, onSortChange }: ModulesSortMenuProps): React.ReactElement {
  const pick = (choice: Partial<ModuleSort>): void => {
    const next = selectModuleSort(sort, choice);
    if (next.key !== sort.key || next.direction !== sort.direction) onSortChange(next);
  };
  const mark = <Check size={14} strokeWidth={2} aria-hidden="true" />;
  const directions = moduleSortHasDirection(sort.key)
    ? [
        { separator: true as const },
        ...MODULE_SORT_DIRECTIONS.map((direction) => ({
          label: MODULE_SORT_DIRECTION_LABELS[direction],
          icon: sort.direction === direction ? mark : undefined,
          onSelect: () => pick({ direction }),
        })),
      ]
    : [];

  return (
    <Menu
      trigger={
        <Button
          variant="secondary"
          size="small"
          data-testid="modules-sort-trigger"
          aria-label={`Order by ${MODULE_SORT_LABELS[sort.key]}`}
        >
          <ArrowDownWideNarrow size={14} strokeWidth={2} aria-hidden="true" />
          <span data-testid="modules-sort-label">{MODULE_SORT_LABELS[sort.key]}</span>
        </Button>
      }
      items={[
        ...MODULE_SORT_OPTIONS.map((option) => ({
          label: option.label,
          icon: sort.key === option.key ? mark : undefined,
          onSelect: () => pick({ key: option.key }),
        })),
        ...directions,
      ]}
    />
  );
}
