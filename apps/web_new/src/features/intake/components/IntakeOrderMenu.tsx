// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake order menu (parity/specs/intake.md INT-011). Three sort fields
// plus the two directions; the chosen field and direction are marked,
// and every pick re-sorts server-side through `order_by`. Field picks
// leave the menu open so the direction can follow in one visit.

import { Button, Menu, type MenuEntry } from "@pidash/kit";
import { ArrowUpDown, Check } from "lucide-react";
import * as React from "react";

import {
  INTAKE_ORDER_DIRECTION_LABELS,
  INTAKE_ORDER_DIRECTIONS,
  INTAKE_ORDER_FIELD_LABELS,
  INTAKE_ORDER_FIELDS,
  type IntakeOrder,
  type IntakeOrderDirection,
  type IntakeOrderField,
} from "../lib/order.js";

export interface IntakeOrderMenuProps {
  order: IntakeOrder;
  onChange: (order: IntakeOrder) => void;
}

function MarkedLabel({
  label,
  checked,
  testId,
}: {
  label: string;
  checked: boolean;
  testId: string;
}): React.ReactElement {
  return (
    <span
      data-testid={testId}
      data-order-checked={checked ? "true" : "false"}
      className="flex items-center gap-(--space-2)"
    >
      <span aria-hidden="true" className="inline-flex w-4 shrink-0 items-center">
        {checked ? <Check size={14} strokeWidth={2.5} /> : null}
      </span>
      <span>{label}</span>
    </span>
  );
}

export function IntakeOrderMenu({ order, onChange }: IntakeOrderMenuProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const pickField = (field: IntakeOrderField): void => onChange({ field, direction: order.direction });
  const pickDirection = (direction: IntakeOrderDirection): void => onChange({ field: order.field, direction });
  const items: MenuEntry[] = [
    ...INTAKE_ORDER_FIELDS.map((field) => ({
      label: INTAKE_ORDER_FIELD_LABELS[field],
      closeOnClick: false,
      onSelect: () => pickField(field),
      content: (
        <MarkedLabel
          label={INTAKE_ORDER_FIELD_LABELS[field]}
          checked={order.field === field}
          testId={`intake-order-field-${field}`}
        />
      ),
    })),
    { separator: true },
    ...INTAKE_ORDER_DIRECTIONS.map((direction) => ({
      label: INTAKE_ORDER_DIRECTION_LABELS[direction],
      onSelect: () => pickDirection(direction),
      content: (
        <MarkedLabel
          label={INTAKE_ORDER_DIRECTION_LABELS[direction]}
          checked={order.direction === direction}
          testId={`intake-order-direction-${direction}`}
        />
      ),
    })),
  ];
  return (
    <Menu
      open={open}
      onOpenChange={setOpen}
      items={items}
      trigger={
        <Button
          variant="ghost"
          size="small"
          data-testid="intake-order-trigger"
          aria-label={`Sort: ${INTAKE_ORDER_FIELD_LABELS[order.field]}, ${INTAKE_ORDER_DIRECTION_LABELS[order.direction]}`}
        >
          <ArrowUpDown size={14} strokeWidth={2} aria-hidden="true" />
          <span>{INTAKE_ORDER_FIELD_LABELS[order.field]}</span>
        </Button>
      }
    />
  );
}
