// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Narrow-viewport request header (parity/specs/intake.md INT-030). The
// detail's action surface below the shared breakpoint: the list toggle,
// previous/next chevrons and the status chip on the left, the header's
// own overflow menu on the right. The bar sticks above the list overlay
// so the toggle and the menu stay reachable while the list is open, and
// the menu trigger is always present — a viewer outside the triage gate
// simply opens an empty menu on an open request.

import { IconButton, Menu, type MenuEntry } from "@pidash/kit";
import { ChevronDown, ChevronUp, MoreHorizontal, PanelLeft } from "lucide-react";
import * as React from "react";

import { NEXT_REQUEST_LABEL, PREV_REQUEST_LABEL, REQUEST_ACTIONS_LABEL } from "./IntakeDetailHeader.js";
import { IntakeStatusChip } from "./IntakeStatusChip.js";

/** Accessible name of the list toggle. */
export const MOBILE_LIST_TOGGLE_LABEL = "Request list";

export interface IntakeMobileHeaderProps {
  /** The list overlay is open; the toggle reports it as expanded. */
  listOpen: boolean;
  onToggleList: () => void;
  onPrev: () => void;
  onNext: () => void;
  /** Intake status for the chip; the chip hides while unknown. */
  status?: number;
  /** Snooze instant for the chip's countdown, if the request is snoozed. */
  snoozedTill?: string | null;
  /** The header's own overflow items, built with the desktop gates. */
  menuItems: MenuEntry[];
}

export function IntakeMobileHeader({
  listOpen,
  onToggleList,
  onPrev,
  onNext,
  status,
  snoozedTill = null,
  menuItems,
}: IntakeMobileHeaderProps): React.ReactElement {
  return (
    <div
      data-testid="intake-mobile-header"
      className="sticky top-0 z-20 flex shrink-0 items-center justify-between gap-(--space-2) border-b border-(--border) bg-(--bg) px-(--space-3) py-(--space-2)"
    >
      <div className="flex items-center gap-(--space-1)">
        <span data-testid="intake-mobile-list-toggle">
          <IconButton
            label={MOBILE_LIST_TOGGLE_LABEL}
            variant="ghost"
            size="small"
            aria-expanded={listOpen}
            onClick={onToggleList}
          >
            <PanelLeft size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        </span>
        <span data-testid="intake-mobile-prev">
          <IconButton label={PREV_REQUEST_LABEL} variant="ghost" size="small" onClick={onPrev}>
            <ChevronUp size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        </span>
        <span data-testid="intake-mobile-next">
          <IconButton label={NEXT_REQUEST_LABEL} variant="ghost" size="small" onClick={onNext}>
            <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        </span>
        {status === undefined ? null : <IntakeStatusChip status={status} snoozedTill={snoozedTill} />}
      </div>
      <div className="flex items-center gap-(--space-2)">
        <span data-testid="intake-mobile-menu">
          <Menu
            trigger={
              <IconButton label={REQUEST_ACTIONS_LABEL} variant="ghost" size="small">
                <MoreHorizontal size={16} strokeWidth={1.5} aria-hidden="true" />
              </IconButton>
            }
            items={menuItems}
          />
        </span>
      </div>
    </div>
  );
}
