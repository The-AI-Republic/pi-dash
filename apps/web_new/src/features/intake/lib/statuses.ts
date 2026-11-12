// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake status vocabulary for the list surface (parity/specs/intake.md
// INT-006, INT-009). The row chip, the filter options and the applied
// chips share these labels, so a status reads the same everywhere. The
// option set is tab-dependent: Open offers Pending and Snoozed, Closed
// the three resolved statuses in the tab's own wire order.

import { INTAKE_STATUS, type IntakeStatus } from "@pidash/api-client";

import type { IntakeTab } from "./tabs.js";

/** Display label per intake status, shared by rows, options and chips. */
export const INTAKE_STATUS_LABELS: Readonly<Record<IntakeStatus, string>> = {
  [INTAKE_STATUS.pending]: "Pending",
  [INTAKE_STATUS.declined]: "Declined",
  [INTAKE_STATUS.snoozed]: "Snoozed",
  [INTAKE_STATUS.accepted]: "Accepted",
  [INTAKE_STATUS.duplicate]: "Duplicate",
};

/**
 * Label for a row's status, or null when the row carries no chip: pending
 * rows never show one (INT-006), and neither do statuses outside the
 * five the screen knows.
 */
export function intakeStatusChipLabel(status: number): string | null {
  if (status === INTAKE_STATUS.pending) return null;
  return (INTAKE_STATUS_LABELS as Readonly<Record<number, string>>)[status] ?? null;
}

/**
 * The status options a tab's filter section offers (INT-009), in display
 * order. The Closed order doubles as the wire order for the `status`
 * parameter.
 */
export const INTAKE_STATUS_OPTIONS: Readonly<Record<IntakeTab, readonly IntakeStatus[]>> = {
  open: [INTAKE_STATUS.pending, INTAKE_STATUS.snoozed],
  closed: [INTAKE_STATUS.accepted, INTAKE_STATUS.declined, INTAKE_STATUS.duplicate],
};

/** The status options a tab's filter section offers. */
export function statusOptionsForTab(tab: IntakeTab): readonly IntakeStatus[] {
  return INTAKE_STATUS_OPTIONS[tab];
}

/**
 * Order a status selection the way the tab lists it, so the `status`
 * parameter is reproducible no matter in which order the user picked.
 * Values outside the tab's option set keep their relative order at the
 * end instead of being dropped.
 */
export function orderStatusesForTab(tab: IntakeTab, statuses: readonly IntakeStatus[]): IntakeStatus[] {
  const options = statusOptionsForTab(tab);
  const rank = new Map<IntakeStatus, number>(options.map((status, index) => [status, index]));
  return [...statuses].sort((left, right) => (rank.get(left) ?? options.length) - (rank.get(right) ?? options.length));
}
