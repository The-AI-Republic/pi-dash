// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The overlay's own presentation switcher (SPC-041). One menu control in
// the header offers the three presentations; the chosen one is component
// state, never an address parameter, so switching keeps the open issue and
// the whole board position untouched.

import { IconButton, Menu, type MenuEntry } from "@pidash/kit";
import { Check, Maximize2, PanelRight, Square } from "lucide-react";
import * as React from "react";

import { spaceMessage } from "../lib/spaceMessages.js";

/** The three presentations the public overlay offers. */
export const SPACE_PEEK_PRESENTATIONS = ["side", "modal", "full"] as const;
export type SpacePeekPresentation = (typeof SPACE_PEEK_PRESENTATIONS)[number];

/** Its human label, read through the space message catalog. */
export function spacePeekPresentationLabel(presentation: SpacePeekPresentation): string {
  switch (presentation) {
    case "side":
      return spaceMessage("peek.mode.side");
    case "modal":
      return spaceMessage("peek.mode.modal");
    case "full":
      return spaceMessage("peek.mode.full");
  }
}

const PRESENTATION_ICONS: Record<SpacePeekPresentation, React.ReactNode> = {
  side: <PanelRight size={16} strokeWidth={1.5} aria-hidden="true" />,
  modal: <Square size={16} strokeWidth={1.5} aria-hidden="true" />,
  full: <Maximize2 size={16} strokeWidth={1.5} aria-hidden="true" />,
};

export interface SpacePeekModeSwitchProps {
  value: SpacePeekPresentation;
  onChange: (presentation: SpacePeekPresentation) => void;
}

export function SpacePeekModeSwitch({ value, onChange }: SpacePeekModeSwitchProps): React.ReactElement {
  const items: MenuEntry[] = SPACE_PEEK_PRESENTATIONS.map((presentation) => ({
    label: spacePeekPresentationLabel(presentation),
    onSelect: () => onChange(presentation),
    icon:
      presentation === value ? (
        <Check size={14} strokeWidth={1.5} aria-hidden="true" />
      ) : (
        <span className="inline-flex w-[14px]" aria-hidden="true" />
      ),
  }));
  return (
    <Menu
      trigger={
        <IconButton label={spaceMessage("peek.presentation")} size="small" data-testid="space-peek-mode-trigger">
          {PRESENTATION_ICONS[value]}
        </IconButton>
      }
      items={items}
    />
  );
}
