// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Rich-filter field picker (parity/specs/issues.md §9, ISS-098). A
// searchable list of fields not already in the expression. Selecting one
// adds a condition joined with AND; once every field is used the list
// carries a single disabled exhaustion row. The trigger is caller-owned so
// the header and the row's add control can share the same picker.

import { Combobox, type ComboboxOption } from "@pidash/kit";
import * as React from "react";

import type { RichFilterField } from "./expression.js";

export interface FieldPickerProps {
  trigger: React.ReactElement<Record<string, unknown>>;
  fields: readonly RichFilterField[];
  /** Field stems already used in the current expression. */
  used: ReadonlySet<string>;
  onPick: (field: RichFilterField) => void;
  open?: boolean | undefined;
  onOpenChange?: ((open: boolean) => void) | undefined;
  /** Accessible label for the popup list. */
  listLabel?: string;
}

/** Build the options, including the disabled exhaustion row. */
export function fieldPickerOptions(fields: readonly RichFilterField[], used: ReadonlySet<string>): ComboboxOption[] {
  const available = fields.filter((field) => !used.has(field.stem));
  if (available.length === 0) {
    return [{ id: "__exhausted__", label: "All filters applied", disabled: true }];
  }
  return available.map((field) => ({ id: field.stem, label: field.label }));
}

export function FieldPicker({
  trigger,
  fields,
  used,
  onPick,
  open,
  onOpenChange,
  listLabel = "Filter properties",
}: FieldPickerProps): React.ReactElement {
  const options = React.useMemo(() => fieldPickerOptions(fields, used), [fields, used]);
  return (
    <Combobox
      trigger={trigger}
      options={options}
      value={null}
      onSelect={(stem) => {
        const field = fields.find((entry) => entry.stem === stem);
        if (field !== undefined) onPick(field);
      }}
      listLabel={listLabel}
      searchLabel="Search properties"
      emptyText="No matching properties"
      open={open}
      onOpenChange={onOpenChange}
    />
  );
}
