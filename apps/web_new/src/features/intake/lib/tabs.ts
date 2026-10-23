// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake tab model (parity/specs/intake.md INT-004, INT-012, INT-028).
// Pure: which tab an address names, the status set each tab reads by
// default, the wire order of that set, and whether the Open badge shows.
// The screen, the route schema and the tests share these rules; nothing
// here touches React, the router or the network.

import { INTAKE_STATUS, type IntakeStatus } from "@pidash/api-client";

/** The two screen tabs, in display order. */
export const INTAKE_TABS = ["open", "closed"] as const;
export type IntakeTab = (typeof INTAKE_TABS)[number];

/** Tab the screen lands on when the address names none. */
export const DEFAULT_INTAKE_TAB: IntakeTab = "open";

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

/**
 * Parse the `currentTab` search value (INT-028). A valid value wins;
 * anything else — unknown, empty, non-string, absent — reads as the
 * default tab, so a malformed address never opens Closed.
 */
export function parseIntakeTab(raw: unknown): IntakeTab {
  return raw === "open" || raw === "closed" ? raw : DEFAULT_INTAKE_TAB;
}

/** The other tab; used by the tab bar's keyboard model. */
export function siblingTab(tab: IntakeTab): IntakeTab {
  return tab === "open" ? "closed" : "open";
}

/** Index of a tab in display order (0 for Open). */
export function tabIndex(tab: IntakeTab): number {
  return INTAKE_TABS.indexOf(tab);
}
