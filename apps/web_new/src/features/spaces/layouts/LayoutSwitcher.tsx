// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// SPC-020/021: a small segmented switcher offering exactly the layouts this
// board may render and the publisher enabled. Every option is a real button
// (keyboard operable, focus visible) whose pressed state is announced; a
// tooltip names it on hover and on focus. A one-layout board still shows it.

import { IconButton, Tooltip } from "@pidash/kit";
import { List, SquareKanban } from "lucide-react";
import * as React from "react";

import type { SpaceLayout } from "../lib/address.js";

/** Switcher tooltip and accessible label per layout. */
export const SPACE_LAYOUT_LABELS: Record<SpaceLayout, { short: string; full: string }> = {
  list: { short: "List", full: "List layout" },
  kanban: { short: "Board", full: "Board layout" },
};

const GLYPHS: Record<SpaceLayout, typeof List> = {
  list: List,
  kanban: SquareKanban,
};

export interface LayoutSwitcherProps {
  offered: readonly SpaceLayout[];
  active: SpaceLayout;
  onSelect: (layout: SpaceLayout) => void;
}

export function LayoutSwitcher({ offered, active, onSelect }: LayoutSwitcherProps): React.ReactElement | null {
  // Malformed settings are settled to list upstream, so this only guards a
  // caller that offers nothing.
  if (offered.length === 0) return null;
  return (
    <div
      role="group"
      aria-label="Board layout"
      data-testid="space-layout-switcher"
      className="flex items-center gap-(--space-1) rounded-(--radius-control) bg-(--subtle) p-(--space-1)"
    >
      {offered.map((layout) => {
        const isActive = layout === active;
        const labels = SPACE_LAYOUT_LABELS[layout];
        const Glyph = GLYPHS[layout];
        return (
          <Tooltip key={layout} content={labels.short}>
            <IconButton
              label={labels.full}
              title={labels.short}
              size="small"
              aria-pressed={isActive}
              data-testid="space-layout-option"
              data-layout={layout}
              data-active={isActive ? "true" : undefined}
              onClick={() => onSelect(layout)}
              className={isActive ? "bg-(--bg) text-(--text)" : undefined}
            >
              <Glyph size={14} strokeWidth={1.75} aria-hidden="true" />
            </IconButton>
          </Tooltip>
        );
      })}
    </div>
  );
}
