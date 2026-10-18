// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One rich-filter condition (parity/specs/issues.md §9, ISS-097/099).
// The pill carries the property, the operator, the value slot and a
// remove control. Changing the property re-derives operator and value;
// unknown properties render as an error pill with a tooltip and only the
// remove action. The operator menu offers positive and negated forms and
// locks when the field advertises a single lookup.

import { Button, Combobox, Tooltip, type ComboboxOption } from "@pidash/kit";
import { AlertCircle, ChevronDown, X } from "lucide-react";
import * as React from "react";

import {
  findField,
  isOperatorLocked,
  isUnresolvableCondition,
  operatorLabel,
  operatorOptions,
  payloadOperator,
  replaceConditionProperty,
  type OperatorOption,
  type RichCondition,
  type RichFilterField,
} from "./expression.js";
import { FieldPicker } from "./FieldPicker.js";
import { ValueSlot } from "./ValueSlot.js";

export interface ConditionPillProps {
  workspaceSlug: string;
  projectId: string;
  fields: readonly RichFilterField[];
  field: RichFilterField | null;
  condition: RichCondition;
  used: ReadonlySet<string>;
  /** All conditions in the row, for property re-picking. */
  conditions: readonly RichCondition[];
  onChange: (next: RichCondition) => void;
  onRemove: () => void;
}

export function ConditionPill({
  workspaceSlug,
  projectId,
  fields,
  field,
  condition,
  used,
  conditions,
  onChange,
  onRemove,
}: ConditionPillProps): React.ReactElement {
  const unresolvable = isUnresolvableCondition(condition, fields);
  const [propertyOpen, setPropertyOpen] = React.useState(false);

  if (unresolvable) {
    return (
      <span
        data-testid="rich-condition-invalid"
        className="inline-flex items-center gap-(--space-1) rounded-(--radius-full) border border-(--danger) bg-(--bg) px-(--space-2) py-(--space-1)"
      >
        <Tooltip content="This filter refers to a property that is no longer available.">
          <span className="text-caption inline-flex items-center gap-(--space-1) text-(--danger)">
            <AlertCircle size={12} strokeWidth={2} aria-hidden="true" />
            Unavailable filter
          </span>
        </Tooltip>
        <button
          type="button"
          aria-label="Remove filter"
          onClick={onRemove}
          className="inline-flex items-center rounded-(--radius-full) text-(--text-muted) hover:text-(--text)"
        >
          <X size={12} strokeWidth={1.5} aria-hidden="true" />
        </button>
      </span>
    );
  }

  const resolved = field as RichFilterField;
  const operatorChoices = operatorOptions(resolved);
  const locked = isOperatorLocked(resolved);

  function changeOperator(option: OperatorOption): void {
    onChange({ ...condition, operator: option.base, negated: option.negated });
  }

  const operatorComboboxOptions: ComboboxOption[] = operatorChoices.map((option) => ({
    id: `${option.base}:${option.negated ? "not" : "yes"}`,
    label: option.label,
    disabled: locked,
  }));

  return (
    <span
      data-testid="rich-condition"
      className="inline-flex items-center gap-(--space-1) rounded-(--radius-full) border border-(--border) bg-(--bg) py-(--space-1) pr-(--space-1) pl-(--space-2)"
    >
      <FieldPicker
        fields={fields}
        used={used}
        open={propertyOpen}
        onOpenChange={setPropertyOpen}
        trigger={
          <Button variant="ghost" size="small">
            {resolved.label}
            <ChevronDown size={12} strokeWidth={2} aria-hidden="true" />
          </Button>
        }
        onPick={(picked) => {
          const next = replaceConditionProperty(conditions, condition.key, picked).find(
            (entry) => entry.key === condition.key
          );
          if (next !== undefined) onChange(next);
          setPropertyOpen(false);
        }}
      />
      <Combobox
        trigger={
          <Button variant="ghost" size="small" disabled={locked}>
            {operatorLabel(payloadOperator(condition.operator, condition.value), condition.negated)}
            <ChevronDown size={12} strokeWidth={2} aria-hidden="true" />
          </Button>
        }
        options={operatorComboboxOptions}
        value={`${condition.operator}:${condition.negated ? "not" : "yes"}`}
        onSelect={(id) => {
          const picked = operatorChoices.find((option) => `${option.base}:${option.negated ? "not" : "yes"}` === id);
          if (picked !== undefined) changeOperator(picked);
        }}
        listLabel={`${resolved.label} operators`}
        searchLabel="Search operators"
        emptyText="No matching operators"
        disabled={locked}
      />
      <ValueSlot
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        field={resolved}
        condition={condition}
        onChange={(value) => onChange({ ...condition, value })}
      />
      <button
        type="button"
        aria-label="Remove filter"
        onClick={onRemove}
        className="inline-flex items-center rounded-(--radius-full) p-(--space-1) text-(--text-muted) hover:text-(--text)"
      >
        <X size={12} strokeWidth={1.5} aria-hidden="true" />
      </button>
    </span>
  );
}

/** Resolve a condition's field, if any. */
export function conditionField(condition: RichCondition, fields: readonly RichFilterField[]): RichFilterField | null {
  return findField(fields, condition.property);
}
