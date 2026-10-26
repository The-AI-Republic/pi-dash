// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The options menu's two display toggles (parity/specs/pages.md §7.4,
// §9.1, PAGE-060/072). Full width and Sticky toolbar lead the owner order;
// each is a checkable row over the local preference store, so selecting it
// writes one preference and leaves the other three untouched. Size and
// face deliberately have no entry here (spec §9.1).

import type { MenuAction } from "@pidash/kit";
import { Check } from "lucide-react";
import * as React from "react";

import type { PagePreferences } from "./pagePreferences.js";

export const PAGE_FULL_WIDTH_LABEL = "Full width";
export const PAGE_STICKY_TOOLBAR_LABEL = "Sticky toolbar";

/** One checkable menu row; a checked entry wears a leading tick. */
function toggle(label: string, checked: boolean, onSelect: () => void): MenuAction {
  return {
    label,
    onSelect,
    ...(checked ? { icon: <Check size={14} strokeWidth={2} aria-hidden="true" /> } : {}),
  };
}

/** The two display rows, in the pinned order, over the current values. */
export function pageDisplayMenuItems(
  preferences: PagePreferences,
  onChange: (patch: Partial<PagePreferences>) => void
): MenuAction[] {
  return [
    toggle(PAGE_FULL_WIDTH_LABEL, preferences.full_width, () => onChange({ full_width: !preferences.full_width })),
    toggle(PAGE_STICKY_TOOLBAR_LABEL, preferences.sticky_toolbar, () =>
      onChange({ sticky_toolbar: !preferences.sticky_toolbar })
    ),
  ];
}
