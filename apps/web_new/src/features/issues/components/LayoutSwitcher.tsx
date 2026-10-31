// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Header layout switcher (parity/specs/issues.md §1, ISS-001). A segmented
// row of icon-only buttons on wide containers collapsing to an icon-plus
// label dropdown on narrow ones. Both variants render (CSS decides which
// shows) and offer the registered layouts in canonical order; clicking
// the active layout is a no-op.

import { IconButton, Menu } from "@pidash/kit";
import * as React from "react";

import { LAYOUT_LABELS, LAYOUT_ORDER, type LayoutKind } from "../layouts/kinds.js";
import { LayoutIcon } from "./LayoutIcon.js";
import { layoutMenuItems } from "./layoutMenuItems.js";

export interface LayoutSwitcherProps {
  /** Registered layouts; the switcher offers them in canonical order. */
  offered: readonly LayoutKind[];
  active: LayoutKind;
  onSwitch: (kind: LayoutKind) => void;
}

function orderedOffered(offered: readonly LayoutKind[]): LayoutKind[] {
  return LAYOUT_ORDER.filter((kind) => offered.includes(kind));
}

export function LayoutSwitcher({ offered, active, onSwitch }: LayoutSwitcherProps): React.ReactElement | null {
  const kinds = orderedOffered(offered);
  if (kinds.length === 0) return null;
  const activeLabel = LAYOUT_LABELS[active];
  return (
    <>
      <div
        role="group"
        aria-label="Issue layout"
        data-testid="layout-switcher-segmented"
        className="hidden items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) bg-(--subtle) p-(--space-1) lg:inline-flex"
      >
        {kinds.map((kind) => {
          const isActive = kind === active;
          const label = LAYOUT_LABELS[kind];
          return (
            <IconButton
              key={kind}
              label={`${label} layout`}
              title={label}
              size="small"
              aria-pressed={isActive}
              data-layout={kind}
              data-active={isActive ? "true" : undefined}
              onClick={() => {
                if (!isActive) onSwitch(kind);
              }}
              className={isActive ? "bg-(--bg) text-(--text)" : undefined}
            >
              <LayoutIcon kind={kind} />
            </IconButton>
          );
        })}
      </div>
      <div data-testid="layout-switcher-dropdown" className="lg:hidden">
        <Menu
          trigger={
            <IconButton label={`Change issue layout, current ${activeLabel}`} title={activeLabel}>
              <LayoutIcon kind={active} />
            </IconButton>
          }
          items={layoutMenuItems(kinds, active, onSwitch)}
        />
      </div>
    </>
  );
}
