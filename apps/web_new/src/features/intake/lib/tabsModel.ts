// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake tab model (parity/specs/intake.md INT-004, INT-012). What each
// tab means once the address has named one: its label, the status set
// it reads by default, the wire order of that set, and whether the
// Open badge shows. The address vocabulary (tabs.ts) stays eager for
// the route's validation; this module rides the screen's chunk, so the
// first paint never pays for tab rules it never calls (NEWFRONT-870,
// size gate). Pure: the screen, the reads and the tests share it.

import { INTAKE_STATUS, type IntakeStatus } from "@pidash/api-client";

import { INTAKE_TABS, type IntakeTab } from "./tabs.js";

/** Visible label of each tab. */
export const INTAKE_TAB_LABELS: Readonly<Record<IntakeTab, string>> = {
  open: "Open",
  closed: "Closed",
};

/**
 * Default status set per tab (INT-012). Open is Pending only — Snoozed
 * belongs to the Open vocabulary but enters through the status filter,
 * never the default read. Closed is the three resolved statuses.
 */
export const INTAKE_TAB_STATUSES: Readonly<Record<IntakeTab, readonly IntakeStatus[]>> = {
  open: [INTAKE_STATUS.pending],
  closed: [INTAKE_STATUS.accepted, INTAKE_STATUS.declined, INTAKE_STATUS.duplicate],
};

/**
 * The status set a tab reads through (INT-012). A tab session's status
 * selection replaces its default, but this slice reads the default: the
 * filter panel that changes the selection is slice B (INT-009).
 */
export function statusSetForTab(tab: IntakeTab): readonly IntakeStatus[] {
  return INTAKE_TAB_STATUSES[tab];
}

/**
 * The `status` query value for a tab: comma-joined in the tab's own
 * order, e.g. `1,-1,2` for Closed. The server's `__in` ignores order;
 * the wire order is pinned so the read is reproducible in tests.
 */
export function statusParamForTab(tab: IntakeTab): string {
  return statusSetForTab(tab).join(",");
}

/**
 * The Open badge is the active Open read's total (INT-004). It renders
 * only while Open is active; Closed carries no badge.
 */
export function shouldShowOpenBadge(tab: IntakeTab): boolean {
  return tab === "open";
}

/** The other tab; used by the tab bar's keyboard model. */
export function siblingTab(tab: IntakeTab): IntakeTab {
  return tab === "open" ? "closed" : "open";
}

/** Index of a tab in display order (0 for Open). */
export function tabIndex(tab: IntakeTab): number {
  return INTAKE_TABS.indexOf(tab);
}
