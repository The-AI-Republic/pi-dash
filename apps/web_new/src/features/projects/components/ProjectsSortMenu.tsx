// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Projects-list sort menu (SHELL-028). The trigger names the current
// key; the menu offers the four keys plus the direction switch, which
// locks while manual order applies. Written from parity/specs/shell.md
// §3; the area gate drives it by visible labels.

import { Button, Menu } from "@pidash/kit";
import { ArrowDownWideNarrow, Check } from "lucide-react";
import * as React from "react";

import {
  LIST_SORT_OPTIONS,
  sortDirectionLocked,
  sortKeyLabel,
  type ListSort,
  type ListSortDirection,
  type ListSortKey,
} from "../lib/list-display.js";

export interface ProjectsSortMenuProps {
  sort: ListSort;
  onSortChange: (sort: ListSort) => void;
}

export function ProjectsSortMenu({ sort, onSortChange }: ProjectsSortMenuProps): React.ReactElement {
  const locked = sortDirectionLocked(sort);

  const pickKey = (key: ListSortKey): void => {
    if (key === sort.key) return;
    if (key === "manual") {
      onSortChange({ key, direction: "ascending" });
      return;
    }
    onSortChange({ key, direction: sort.direction });
  };

  const pickDirection = (direction: ListSortDirection): void => {
    if (locked || direction === sort.direction) return;
    onSortChange({ key: sort.key, direction });
  };

  const mark = <Check size={14} strokeWidth={2} aria-hidden="true" />;

  return (
    <Menu
      trigger={
        <Button variant="secondary" size="small" data-testid="projects-sort-trigger">
          <ArrowDownWideNarrow size={14} strokeWidth={2} aria-hidden="true" />
          {sortKeyLabel(sort.key)}
        </Button>
      }
      items={[
        ...LIST_SORT_OPTIONS.map((option) => ({
          label: option.label,
          icon: sort.key === option.key ? mark : undefined,
          onSelect: () => pickKey(option.key),
        })),
        { separator: true },
        {
          label: "Ascending",
          icon: !locked && sort.direction === "ascending" ? mark : undefined,
          disabled: locked,
          onSelect: () => pickDirection("ascending"),
        },
        {
          label: "Descending",
          icon: !locked && sort.direction === "descending" ? mark : undefined,
          disabled: locked,
          onSelect: () => pickDirection("descending"),
        },
      ]}
    />
  );
}
