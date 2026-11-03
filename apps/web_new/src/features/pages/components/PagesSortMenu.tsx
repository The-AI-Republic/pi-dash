// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages sort control (parity/specs/pages.md §3.3, PAGE-004). One menu
// lists the three keys, then the two directions; the trigger names the
// active key and carries a direction glyph. Every pick re-sorts the
// already-loaded rows, and re-picking the current direction is inert.

import { Button, Menu } from "@pidash/kit";
import { ArrowDownWideNarrow, ArrowUpNarrowWide } from "lucide-react";
import * as React from "react";

import {
  PAGE_SORT_DIRECTIONS,
  PAGE_SORT_KEYS,
  pageSortKeyLabel,
  type PageSort,
  type PageSortKey,
} from "../lib/pageList.js";

export interface PagesSortMenuProps {
  sort: PageSort;
  onSortChange: (sort: PageSort) => void;
}

export function PagesSortMenu({ sort, onSortChange }: PagesSortMenuProps): React.ReactElement {
  const label = pageSortKeyLabel(sort.key);
  const descending = sort.direction === "descending";
  const chooseKey = (key: PageSortKey): void => {
    if (key !== sort.key) onSortChange({ ...sort, key });
  };
  const chooseDirection = (direction: PageSort["direction"]): void => {
    if (direction !== sort.direction) onSortChange({ ...sort, direction });
  };

  return (
    <Menu
      trigger={
        <Button size="small" variant="ghost" aria-label={label} className="text-(--text-muted)">
          <span>{label}</span>
          {descending ? (
            <ArrowDownWideNarrow size={14} strokeWidth={1.5} aria-hidden="true" />
          ) : (
            <ArrowUpNarrowWide size={14} strokeWidth={1.5} aria-hidden="true" />
          )}
        </Button>
      }
      items={[
        ...PAGE_SORT_KEYS.map((option) => ({
          label: option.label,
          onSelect: () => chooseKey(option.key),
        })),
        { separator: true as const },
        ...PAGE_SORT_DIRECTIONS.map((option) => ({
          label: option.label,
          onSelect: () => chooseDirection(option.direction),
        })),
      ]}
    />
  );
}
