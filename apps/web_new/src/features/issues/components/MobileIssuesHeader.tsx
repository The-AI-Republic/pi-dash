// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Compact mobile issues header (parity/specs/issues.md §1, ISS-003). A
// single bar with the layout menu (list, board, calendar) plus the Display
// and analytics controls their slices provide. Shown under md only.

import { Button, Menu } from "@pidash/kit";
import * as React from "react";

import { LAYOUT_LABELS, mobileOfferedLayouts, type LayoutKind } from "../layouts/kinds.js";
import { LayoutIcon } from "./LayoutIcon.js";
import { layoutMenuItems } from "./layoutMenuItems.js";

export interface MobileIssuesHeaderProps {
  /** Registered layouts; the menu offers the mobile subset in order. */
  offered: readonly LayoutKind[];
  active: LayoutKind;
  onSwitch: (kind: LayoutKind) => void;
  /** Display control from the display slice (ISS-075). */
  displayControl?: React.ReactNode;
  /** Rich-filter control from the filter slice (ISS-096). */
  filterControl?: React.ReactNode;
  /** Analytics entry from the filter-inputs slice (ISS-107). */
  analyticsControl?: React.ReactNode;
  /**
   * Hide the layout menu (a locked saved view renders its stored layout
   * with no switcher, parity/specs/views.md §1). With no trailing control
   * either, the bar renders nothing instead of an empty strip.
   */
  hideSwitcher?: boolean | undefined;
}

export function MobileIssuesHeader({
  offered,
  active,
  onSwitch,
  displayControl,
  filterControl,
  analyticsControl,
  hideSwitcher = false,
}: MobileIssuesHeaderProps): React.ReactElement | null {
  const kinds = mobileOfferedLayouts(offered);
  if (hideSwitcher && displayControl === undefined && filterControl === undefined && analyticsControl === undefined) {
    return null;
  }
  return (
    <div
      data-testid="issues-mobile-header"
      className="flex shrink-0 items-center gap-(--space-2) border-b border-(--border) px-(--space-3) py-(--space-2) md:hidden"
    >
      {hideSwitcher ? null : (
        <Menu
          trigger={
            <Button variant="secondary" size="small" data-testid="mobile-layout-trigger">
              <LayoutIcon kind={active} />
              {LAYOUT_LABELS[active]}
            </Button>
          }
          items={layoutMenuItems(kinds, active, onSwitch)}
        />
      )}
      <div className="ml-auto flex items-center gap-(--space-2)">
        {filterControl}
        {displayControl}
        {analyticsControl}
      </div>
    </div>
  );
}
