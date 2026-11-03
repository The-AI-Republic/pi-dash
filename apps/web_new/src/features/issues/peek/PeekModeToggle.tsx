// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Peek display-mode toggle (parity/specs/issues.md §14, ISS-166). The
// header switch between the docked side peek, the centered modal, and
// the full-screen portal. The mode stays component-local: reopening
// always lands back on the side dock, and the URL never names it.

import { IconButton, Menu, type MenuEntry } from "@pidash/kit";
import { Check, Maximize2, PanelRight, Square } from "lucide-react";
import * as React from "react";

import type { PeekLayout } from "../../../shared/shell/PeekPanel.js";

/** Display modes the peek header switches between (ISS-166). */
export type PeekMode = PeekLayout;

export const PEEK_MODES: readonly PeekMode[] = ["side", "modal", "full"];

const MODE_LABELS: Record<PeekMode, string> = {
  side: "Side peek",
  modal: "Modal",
  full: "Full screen",
};

/** Human label for a display mode. */
export function peekModeLabel(mode: PeekMode): string {
  return MODE_LABELS[mode];
}

const MODE_ICONS: Record<PeekMode, React.ReactNode> = {
  side: <PanelRight size={16} strokeWidth={1.5} aria-hidden="true" />,
  modal: <Square size={16} strokeWidth={1.5} aria-hidden="true" />,
  full: <Maximize2 size={16} strokeWidth={1.5} aria-hidden="true" />,
};

export interface PeekModeToggleProps {
  value: PeekMode;
  onChange: (mode: PeekMode) => void;
}

export function PeekModeToggle({ value, onChange }: PeekModeToggleProps): React.ReactElement {
  const items: MenuEntry[] = PEEK_MODES.map((mode) => ({
    label: MODE_LABELS[mode],
    onSelect: () => onChange(mode),
    icon:
      mode === value ? (
        <Check size={14} strokeWidth={1.5} aria-hidden="true" />
      ) : (
        <span className="inline-flex w-[14px]" aria-hidden="true" />
      ),
  }));
  return (
    <Menu
      trigger={
        <IconButton label="Display mode" size="small" data-testid="peek-mode-trigger">
          {MODE_ICONS[value]}
        </IconButton>
      }
      items={items}
    />
  );
}
