// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake tab address vocabulary (parity/specs/intake.md INT-004,
// INT-028). Pure: which tab an address names. What each tab means —
// labels, status sets, the badge — lives in tabsModel.ts on the
// screen's chunk; this module stays eager for the route's validation.

/** The two screen tabs, in display order. */
export const INTAKE_TABS = ["open", "closed"] as const;
export type IntakeTab = (typeof INTAKE_TABS)[number];

/** Tab the screen lands on when the address names none. */
export const DEFAULT_INTAKE_TAB: IntakeTab = "open";

/**
 * Parse the `currentTab` search value (INT-028). A valid value wins;
 * anything else — unknown, empty, non-string, absent — reads as the
 * default tab, so a malformed address never opens Closed.
 */
export function parseIntakeTab(raw: unknown): IntakeTab {
  return raw === "open" || raw === "closed" ? raw : DEFAULT_INTAKE_TAB;
}
