// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Previous/next request movement (parity/specs/intake.md INT-027). Pure:
// the neighbour in display order with wraparound, and the arrow-key
// guard. Arrows stay quiet while typing (title, description, any other
// text surface) and while an overlay is open (dialog, menu, picker
// popup), so they never steal caret movement or popup navigation.

import { isTypingTarget } from "../../../shared/commands/index.js";

export type IntakeStepDirection = "prev" | "next";

/** Overlay roles whose presence in the document suppresses the arrows. */
export const INTAKE_NAV_OVERLAY_SELECTOR = '[role="dialog"], [role="menu"], [role="listbox"]';

/**
 * The id a step lands on: the neighbour in display order, wrapping at
 * the ends. An unknown selection steps onto the first (next) or last
 * (prev) row; an empty list steps nowhere.
 */
export function neighborIntakeId(
  rowIds: readonly string[],
  currentId: string | undefined,
  direction: IntakeStepDirection
): string | null {
  if (rowIds.length === 0) return null;
  const index = currentId === undefined ? -1 : rowIds.indexOf(currentId);
  if (index === -1) return direction === "next" ? rowIds[0]! : rowIds[rowIds.length - 1]!;
  const delta = direction === "next" ? 1 : -1;
  return rowIds[(index + delta + rowIds.length) % rowIds.length]!;
}

/** True when an arrow key must not move between requests. */
export function suppressIntakeArrowKey(target: EventTarget | null, overlayOpen: boolean): boolean {
  if (overlayOpen) return true;
  return isTypingTarget(target);
}
