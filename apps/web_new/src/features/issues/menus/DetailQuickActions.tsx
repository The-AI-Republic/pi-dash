// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail and peek header actions (parity/specs/issues.md §7, ISS-066).
// The header carries the destructive set — duplicate, open, move,
// archive or restore, delete — because the page itself already offers
// editing and its own copy-link control. The peek mounts the same
// control with its own copy button beside it, so both read one menu.

import { IconButton, Menu } from "@pidash/kit";
import { Ellipsis } from "lucide-react";
import * as React from "react";
import { createPortal } from "react-dom";

import { menuItemsFor } from "./RowQuickActions.js";
import type { QuickActionsState } from "./useQuickActions.js";

export interface DetailQuickActionsProps {
  state: QuickActionsState;
  label?: string | undefined;
}

/** `data-main-menu` marks the header's primary menu for the shell. */
export function DetailQuickActions({
  state,
  label = "Work item actions",
}: DetailQuickActionsProps): React.ReactElement {
  return (
    <div data-main-menu="true" className="flex items-center">
      <Menu
        trigger={
          <IconButton label={label} size="small" data-testid="detail-menu-trigger">
            <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        }
        items={menuItemsFor(state)}
      />
      {createPortal(state.overlays, document.body)}
    </div>
  );
}
