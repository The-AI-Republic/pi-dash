// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake list ordering (parity/specs/intake.md INT-011). Three server
// sort fields, each ascending or descending; the default is created
// date, newest first. Ordering is server-side through `order_by`: the
// client never re-sorts locally.

/** Sort fields in menu order. */
export const INTAKE_ORDER_FIELDS = ["created", "updated", "id"] as const;
export type IntakeOrderField = (typeof INTAKE_ORDER_FIELDS)[number];

/** Sort directions in menu order. */
export const INTAKE_ORDER_DIRECTIONS = ["desc", "asc"] as const;
export type IntakeOrderDirection = (typeof INTAKE_ORDER_DIRECTIONS)[number];

export interface IntakeOrder {
  field: IntakeOrderField;
  direction: IntakeOrderDirection;
}

/** The default order: created date, descending (INT-011). */
export const DEFAULT_INTAKE_ORDER_VALUE: IntakeOrder = { field: "created", direction: "desc" };

/** Menu labels for the sort fields. */
export const INTAKE_ORDER_FIELD_LABELS: Readonly<Record<IntakeOrderField, string>> = {
  created: "Created date",
  updated: "Last updated date",
  id: "ID",
};

/** Menu labels for the sort directions. */
export const INTAKE_ORDER_DIRECTION_LABELS: Readonly<Record<IntakeOrderDirection, string>> = {
  asc: "Ascending",
  desc: "Descending",
};

const ORDER_COLUMNS: Readonly<Record<IntakeOrderField, string>> = {
  created: "issue__created_at",
  updated: "issue__updated_at",
  id: "issue__sequence_id",
};

/**
 * The `order_by` wire value for an order: the field's column,
 * `-`-prefixed for descending.
 */
export function orderByForOrder(order: IntakeOrder): string {
  const column = ORDER_COLUMNS[order.field];
  return order.direction === "desc" ? `-${column}` : column;
}
