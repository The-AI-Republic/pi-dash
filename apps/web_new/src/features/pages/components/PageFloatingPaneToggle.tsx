// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Floating pane toggle (parity/specs/pages.md §5.3, PAGE-035). When the
// sticky formatting bar is absent — a read-only page or the sticky
// preference switched off — this control keeps the navigation pane one
// press away at the canvas edge. It is never mounted beside the bar, so
// its name marks the hidden-bar state.

import { IconButton } from "@pidash/kit";
import { PanelRight } from "lucide-react";
import * as React from "react";

export const PAGE_OPEN_PANE_LABEL = "Open navigation pane";
export const PAGE_FLOATING_PANE_TEST_ID = "page-floating-pane-toggle";

export interface PageFloatingPaneToggleProps {
  /** Reveal the navigation pane (PAGE-045). */
  onOpen: () => void;
}

export function PageFloatingPaneToggle({ onOpen }: PageFloatingPaneToggleProps): React.ReactElement {
  return (
    <div className="absolute top-(--space-4) right-(--space-4) z-10">
      <IconButton label={PAGE_OPEN_PANE_LABEL} data-testid={PAGE_FLOATING_PANE_TEST_ID} size="small" onClick={onOpen}>
        <PanelRight size={16} strokeWidth={1.5} aria-hidden="true" />
      </IconButton>
    </div>
  );
}
