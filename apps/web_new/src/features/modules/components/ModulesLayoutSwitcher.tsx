// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Layout switcher (parity/specs/modules.md §3.4 and §13.3.1, MOD-011). An
// icon group on wide containers offering list, board and timeline in
// canonical order, collapsing on small screens to a menu that offers list
// and board only, with the pinned "List layout" and "Gallery layout"
// labels. The active entry is pressed; choosing it again does nothing.
// Switching is pure display state: no read, no write to the server.

import { IconButton, Menu } from "@pidash/kit";
import * as React from "react";

import { useNarrowViewport } from "../../../shared/shell/index.js";
import {
  MODULES_COMPACT_LAYOUT_OPTIONS,
  MODULES_LAYOUT_LABELS,
  MODULES_LAYOUTS,
  resolveCompactLayout,
  type ModulesCompactLayout,
  type ModulesLayout,
} from "../lib/moduleLayouts.js";
import { ModuleLayoutIcon } from "./ModuleLayoutIcon.js";

export interface ModulesLayoutSwitcherProps {
  /** The stored choice for this project. */
  layout: ModulesLayout;
  onLayoutChange: (layout: ModulesLayout) => void;
}

export function ModulesLayoutSwitcher({ layout, onLayoutChange }: ModulesLayoutSwitcherProps): React.ReactElement {
  const narrow = useNarrowViewport();
  // A small screen never offers timeline; the stored choice survives for
  // whenever the same project reopens wide.
  const compact: ModulesCompactLayout = resolveCompactLayout(layout);
  return (
    <>
      {narrow ? null : (
        <div
          role="group"
          aria-label="Module layout"
          data-testid="modules-layout-switcher"
          className="inline-flex items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) bg-(--subtle) p-(--space-1)"
        >
          {MODULES_LAYOUTS.map((option) => {
            const isActive = option === layout;
            const label = MODULES_LAYOUT_LABELS[option];
            return (
              <IconButton
                key={option}
                label={`${label} layout`}
                title={label}
                size="small"
                aria-pressed={isActive}
                data-layout={option}
                data-active={isActive ? "true" : undefined}
                onClick={() => {
                  if (!isActive) onLayoutChange(option);
                }}
                className={isActive ? "bg-(--bg) text-(--text)" : undefined}
              >
                <ModuleLayoutIcon layout={option} />
              </IconButton>
            );
          })}
        </div>
      )}
      {narrow ? (
        <div data-testid="modules-layout-menu">
          <Menu
            trigger={
              <IconButton label={`Change module layout, current ${MODULES_LAYOUT_LABELS[compact]}`} title="Layout">
                <ModuleLayoutIcon layout={compact} />
              </IconButton>
            }
            items={MODULES_COMPACT_LAYOUT_OPTIONS.map((option) => ({
              label: option.label,
              icon: <ModuleLayoutIcon layout={option.layout} />,
              itemKey: option.layout,
              onSelect: () => {
                if (option.layout !== compact) onLayoutChange(option.layout);
              },
            }))}
          />
        </div>
      ) : null}
    </>
  );
}
