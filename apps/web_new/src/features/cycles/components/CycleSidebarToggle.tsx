// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar collapse toggle (parity/specs/cycles.md §14, CYC-035). One
// icon button behind both mounts: the detail header on wide screens and
// the trail row on narrow ones. The label and the expanded state follow
// the panel so assistive tech announces the current side.

import { IconButton } from "@pidash/kit";
import { PanelRight } from "lucide-react";
import * as React from "react";

export interface CycleSidebarToggleProps {
  /** True while the panel is hidden. */
  collapsed: boolean;
  /** Flip the collapse state (the caller persists it). */
  onToggle: () => void;
  /** Surface hook; the narrow mount carries its own test id. */
  testId?: string | undefined;
}

/** Expand the hidden sidebar. */
export const CYCLE_SIDEBAR_EXPAND_LABEL = "Expand sidebar";

/** Collapse the visible sidebar. */
export const CYCLE_SIDEBAR_COLLAPSE_LABEL = "Collapse sidebar";

export function CycleSidebarToggle({
  collapsed,
  onToggle,
  testId = "cycle-sidebar-toggle",
}: CycleSidebarToggleProps): React.ReactElement {
  return (
    <IconButton
      label={collapsed ? CYCLE_SIDEBAR_EXPAND_LABEL : CYCLE_SIDEBAR_COLLAPSE_LABEL}
      aria-expanded={!collapsed}
      data-testid={testId}
      size="small"
      onClick={onToggle}
    >
      <PanelRight size={16} strokeWidth={1.5} aria-hidden="true" />
    </IconButton>
  );
}
