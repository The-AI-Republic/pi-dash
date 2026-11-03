// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Narrow-screen cycles-list presentation menu
// (parity/specs/cycles.md §21, CYC-048). Wide viewports always render
// the list rows, so this control mounts on narrow screens only: a
// menu offering exactly "List layout" and "Gallery layout". Gallery
// re-presents the same rows — same order, same hero and groups — and
// switching is pure display state: no read, no write to the server.

import { Button, Menu } from "@pidash/kit";
import { LayoutGrid, LayoutList } from "lucide-react";
import * as React from "react";

import { useNarrowViewport } from "../../../shared/shell/index.js";
import { CYCLE_LIST_LAYOUT_OPTIONS, cycleListLayoutLabel, type CycleListLayout } from "../lib/cycleListLayout.js";

export interface CycleListLayoutMenuProps {
  /** The current row presentation. */
  layout: CycleListLayout;
  onLayoutChange: (layout: CycleListLayout) => void;
}

export function CycleListLayoutMenu({ layout, onLayoutChange }: CycleListLayoutMenuProps): React.ReactElement | null {
  const narrow = useNarrowViewport();
  if (!narrow) return null;
  const PresentationIcon = layout === "gallery" ? LayoutGrid : LayoutList;
  return (
    <div data-testid="cycles-list-layout-menu" className="flex justify-center">
      <Menu
        trigger={
          <Button variant="secondary" size="small" data-testid="cycles-list-layout-trigger">
            <PresentationIcon size={16} strokeWidth={1.5} aria-hidden="true" />
            {cycleListLayoutLabel(layout)}
          </Button>
        }
        items={CYCLE_LIST_LAYOUT_OPTIONS.map((option) => ({
          label: option.label,
          itemKey: option.layout,
          onSelect: () => {
            if (option.layout !== layout) onLayoutChange(option.layout);
          },
        }))}
      />
    </div>
  );
}
