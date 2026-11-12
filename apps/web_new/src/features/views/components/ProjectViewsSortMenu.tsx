// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Order menu for the project views list (parity/specs/views.md §4,
// VIEW-005). The trigger names the active key; the menu lists the three
// keys, a divider, then the two directions, each marking the active choice.
// The pick is held by the screen only — a reload restores the default — and
// re-picking the current key or direction changes nothing.

import { Button, Menu } from "@pidash/kit";
import { ArrowDownWideNarrow, Check } from "lucide-react";
import * as React from "react";

import {
  selectViewSort,
  VIEW_SORT_DIRECTION_OPTIONS,
  VIEW_SORT_KEY_OPTIONS,
  viewSortLabel,
  type ViewSort,
} from "../lib/viewListControls.js";

export interface ProjectViewsSortMenuProps {
  sort: ViewSort;
  onSortChange: (sort: ViewSort) => void;
}

export function ProjectViewsSortMenu({ sort, onSortChange }: ProjectViewsSortMenuProps): React.ReactElement {
  const pick = (choice: Partial<ViewSort>): void => {
    const next = selectViewSort(sort, choice);
    if (next !== sort) onSortChange(next);
  };
  const mark = <Check size={14} strokeWidth={2} aria-hidden="true" />;

  return (
    <Menu
      trigger={
        <Button variant="secondary" size="small" data-testid="project-views-sort-trigger">
          <ArrowDownWideNarrow size={14} strokeWidth={2} aria-hidden="true" className="hidden sm:inline" />
          {viewSortLabel(sort.key)}
        </Button>
      }
      items={[
        ...VIEW_SORT_KEY_OPTIONS.map((option) => ({
          label: option.label,
          icon: sort.key === option.key ? mark : undefined,
          onSelect: () => pick({ key: option.key }),
        })),
        { separator: true as const },
        ...VIEW_SORT_DIRECTION_OPTIONS.map((option) => ({
          label: option.label,
          icon: sort.direction === option.direction ? mark : undefined,
          onSelect: () => pick({ direction: option.direction }),
        })),
      ]}
    />
  );
}
