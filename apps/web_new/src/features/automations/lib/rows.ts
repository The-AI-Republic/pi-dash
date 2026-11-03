// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row shaping for the automations page (parity/specs/agents.md §8,
// AGT-034/035). Pure selectors only: what a toggle writes, which
// cancelled states the close picker offers, and which state label the
// row shows. The components stay and render; the rules live here so
// every arm is unit-tested without a DOM.

import type { ProjectUpdateInput, State } from "@pidash/api-client";

/** Cancelled-group states, in server order, as the close picker shows them. */
export function selectCancelledStates(states: readonly State[]): State[] {
  return states.filter((state) => state.group === "cancelled");
}

/**
 * Cycling the close toggle writes a one-month delay toward the first
 * cancelled state, or clears both fields. The target is null when the
 * project has no cancelled state, which is allowed by the server.
 */
export function closeTogglePatch(open: boolean, cancelled: readonly State[]): ProjectUpdateInput {
  if (!open) return { close_in: 0, default_state: null };
  return { close_in: 1, default_state: cancelled[0]?.id ?? null };
}

/** Cycling the archive toggle writes a one-month delay or clears it. */
export function archiveTogglePatch(open: boolean): ProjectUpdateInput {
  return { archive_in: open ? 1 : 0 };
}

/**
 * The close row's visible target: the stored state when it is one of the
 * project's cancelled states, otherwise the first cancelled state, and
 * null when the project has none. A stale stored target never leaks a
 * non-cancelled name into the row.
 */
export function selectCloseTarget(
  project: { default_state: string | null } | undefined,
  cancelled: readonly State[]
): State | null {
  if (cancelled.length === 0) return null;
  const stored = project?.default_state ?? null;
  // A stale stored target never wins; otherwise the first cancelled state.
  return cancelled.find((state) => state.id === stored) ?? cancelled[0] ?? null;
}

/** The state picker opens only when there is more than one cancelled state. */
export function isStatePickerEnabled(cancelled: readonly State[]): boolean {
  return cancelled.length > 1;
}
