// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared layout menu entries (ISS-001/ISS-003). The narrow-container
// switcher dropdown and the mobile layout menu offer the same rows:
// icon plus label, a check on the active layout, and a no-op select on
// the active entry.

import { Check } from "lucide-react";
import * as React from "react";
import type { MenuEntry } from "@pidash/kit";

import { LAYOUT_LABELS, type LayoutKind } from "../layouts/kinds.js";
import { LayoutIcon } from "./LayoutIcon.js";

/** Menu entries for the given kinds in the given order. */
export function layoutMenuItems(
  kinds: readonly LayoutKind[],
  active: LayoutKind,
  onSwitch: (kind: LayoutKind) => void
): MenuEntry[] {
  return kinds.map((kind) => ({
    label: LAYOUT_LABELS[kind],
    itemKey: kind,
    icon: <LayoutIcon kind={kind} />,
    content:
      kind === active ? (
        <span className="flex w-full items-center gap-(--space-2)">
          {LAYOUT_LABELS[kind]}
          <Check size={14} strokeWidth={2} aria-hidden="true" className="ml-auto text-(--text-muted)" />
        </span>
      ) : undefined,
    onSelect: () => {
      if (kind !== active) onSwitch(kind);
    },
  }));
}
