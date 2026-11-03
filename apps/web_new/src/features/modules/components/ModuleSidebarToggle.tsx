// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar collapse toggle (parity/specs/modules.md §8.4, MOD-032). The
// detail header mounts it beside the analytics entry. The label and the
// expanded state follow the panel so assistive tech announces the
// current side.

import { IconButton } from "@pidash/kit";
import { PanelRight } from "lucide-react";
import * as React from "react";

export interface ModuleSidebarToggleProps {
  /** True while the panel is hidden. */
  collapsed: boolean;
  /** Flip the collapse state (the caller persists it). */
  onToggle: () => void;
}

export const MODULE_SIDEBAR_EXPAND_LABEL = "Expand sidebar";

export const MODULE_SIDEBAR_COLLAPSE_LABEL = "Collapse sidebar";

export function ModuleSidebarToggle({ collapsed, onToggle }: ModuleSidebarToggleProps): React.ReactElement {
  return (
    <IconButton
      label={collapsed ? MODULE_SIDEBAR_EXPAND_LABEL : MODULE_SIDEBAR_COLLAPSE_LABEL}
      aria-expanded={!collapsed}
      data-testid="module-detail-sidebar-toggle"
      size="small"
      onClick={onToggle}
    >
      <PanelRight size={16} strokeWidth={1.5} aria-hidden="true" />
    </IconButton>
  );
}
