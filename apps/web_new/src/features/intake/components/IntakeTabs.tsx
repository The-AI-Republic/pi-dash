// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Open/Closed tab bar (parity/specs/intake.md INT-004). Real tab
// semantics: a tablist owns two tabs, the active one carries
// aria-selected and points at the list panel, and the Open tab's
// accessible name carries its live total ("Open, 3 pending"). Closed
// never shows a badge. Arrow keys move between tabs; activation is the
// click (or Enter/Space, which the button gives for free).

import * as React from "react";

import { INTAKE_TABS, type IntakeTab } from "../lib/tabs.js";
import { INTAKE_TAB_LABELS, siblingTab, shouldShowOpenBadge } from "../lib/tabsModel.js";

/** Id of the list panel the tabs control; the list pane renders it. */
export const INTAKE_LIST_PANEL_ID = "intake-list-panel";

export interface IntakeTabsProps {
  activeTab: IntakeTab;
  /** The active Open read's `total_results`; ignored on Closed. */
  openTotal: number | undefined;
  onSelect: (tab: IntakeTab) => void;
}

export function openTabAccessibleName(total: number | undefined): string {
  return total === undefined ? INTAKE_TAB_LABELS.open : `${INTAKE_TAB_LABELS.open}, ${total} pending`;
}

export function IntakeTabs({ activeTab, openTotal, onSelect }: IntakeTabsProps): React.ReactElement {
  const refs = React.useRef<Record<IntakeTab, HTMLButtonElement | null>>({ open: null, closed: null });
  const move = (tab: IntakeTab) => (event: React.KeyboardEvent<HTMLButtonElement>) => {
    if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") return;
    event.preventDefault();
    refs.current[siblingTab(tab)]?.focus();
  };
  return (
    <div role="tablist" aria-label="Intake requests" data-testid="intake-tabs" className="flex gap-(--space-1)">
      {INTAKE_TABS.map((tab) => {
        const selected = tab === activeTab;
        const badge = selected && shouldShowOpenBadge(tab) ? openTotal : undefined;
        return (
          <button
            key={tab}
            ref={(node) => {
              refs.current[tab] = node;
            }}
            type="button"
            role="tab"
            data-testid={`intake-tab-${tab}`}
            aria-selected={selected}
            aria-controls={INTAKE_LIST_PANEL_ID}
            tabIndex={selected ? 0 : -1}
            aria-label={tab === "open" ? openTabAccessibleName(badge) : INTAKE_TAB_LABELS.closed}
            onClick={() => onSelect(tab)}
            onKeyDown={move(tab)}
            className={
              selected
                ? "text-body rounded-(--radius-control) bg-(--subtle) px-(--space-2) py-(--space-1) font-medium text-(--text)"
                : "text-body rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) hover:text-(--text)"
            }
          >
            <span>{INTAKE_TAB_LABELS[tab]}</span>
            {badge === undefined ? null : (
              <span data-testid="intake-open-count" aria-hidden="true" className="ml-(--space-1) text-(--text-muted)">
                {badge}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}
